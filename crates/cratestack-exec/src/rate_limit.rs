//! Rate-limit admission — ADR 0015 slice 2 (cratestack#877).
//!
//! What moved here, and what deliberately did not:
//!
//! - **Moved:** the rule for *which ops are rate limited at all*
//!   ([`OpAdmission::rate_limited_by_default`], the `@no_rate_limit`
//!   exemption cratestack#474 made live), the rule for *what to do when the
//!   op could not be identified* (charge it — see below), and the store
//!   call itself.
//! - **Stayed at the transport:** deriving the bucket key and its budget
//!   (they read `Authorization`, `ConnectInfo` and a verified-principal
//!   extension — all transport facts, exactly like the idempotency
//!   fingerprint in slice 1), enforcing the lookup timeout, applying the
//!   store-error policy, and every response the limiter renders. Those are
//!   cratestack#846 and cratestack#871's decisions, carried across
//!   verbatim rather than re-decided under a refactor. The policy's *type*
//!   and its rule did move later, to `crate::store_error`, so the
//!   transports share one (cratestack#1038).
//!
//! # Why this is not a new [`crate::Admission`] variant
//!
//! Slice 1's doc anticipated rate limiting arriving as another variant of
//! the same enum. It cannot, without changing what an admitted request
//! looks like on the wire: an *allowed* rate-limit decision still carries
//! data the response needs (`X-RateLimit-Remaining`), and which bucket was
//! charged (the cratestack#871 amplification log). `Admission::Bypass` and
//! `Admission::Reserved` have nowhere to put either, and giving them a
//! field would break every published `match` on them. So the two concerns
//! get two answers from one executor — which is also how they are mounted
//! today: two independent `tower::Layer`s, each asking only its own
//! question.

use std::sync::Arc;

use cratestack_core::{
    BoundedOutcome, BucketBudget, ConsumeRequest, CratestackError, RateLimitConfig, RateLimitStore,
};

use crate::executor::OpExecutor;
use crate::input::{OpAdmission, OpInput};

/// The store and the token-bucket parameters an executor rate-limits with.
/// Named collaborators, supplied at construction (ADR 0012).
#[derive(Clone)]
pub(crate) struct RateLimiter {
    store: Arc<dyn RateLimitStore>,
    config: RateLimitConfig,
}

/// The bucket one call is charged against, already derived by the caller.
///
/// Derivation is a transport concern — see this module's doc — so it
/// arrives computed, the same way [`OpInput::fingerprint`] does.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct RateLimitBucket<'a> {
    /// The bucket the caller asked to be charged.
    pub key: &'a str,
    /// How many distinct buckets the key's scope may create, and what to
    /// charge instead once it is full (cratestack#871). `None` for a key
    /// that is not caller-mintable, e.g. one built from a verified
    /// principal.
    pub budget: Option<&'a BucketBudget>,
}

impl<'a> RateLimitBucket<'a> {
    pub fn new(key: &'a str, budget: Option<&'a BucketBudget>) -> Self {
        Self { key, budget }
    }
}

/// Outcome of [`OpExecutor::admit_rate_limit`].
///
/// # `#[non_exhaustive]`
///
/// A caller's wildcard arm must refuse, never admit — the same rule
/// `cratestack-axum`'s idempotency adapter follows for [`crate::Admission`].
#[non_exhaustive]
pub enum RateLimitAdmission {
    /// Not rate limited: no store is wired, or the op declared
    /// `@no_rate_limit`. Nothing was charged.
    Bypass,
    /// One token was consumed. The store's answer is passed through
    /// untouched — the decision (allowed with `remaining`, or throttled
    /// with `retry_after_secs`) and which bucket paid — so a transport
    /// renders it exactly as it rendered the store's answer before this
    /// crate existed.
    Consumed(BoundedOutcome),
}

impl OpExecutor {
    /// Rate-limit calls with `store`, using `config`'s token bucket.
    ///
    /// Without this, [`Self::admit_rate_limit`] bypasses every call — the
    /// same "nothing wired admits everything" rule the idempotency store
    /// follows, and for the same reason: a service with no limiter
    /// configured is not an error.
    pub fn with_rate_limit(
        mut self,
        store: Arc<dyn RateLimitStore>,
        config: RateLimitConfig,
    ) -> Self {
        self.rate_limit = Some(RateLimiter { store, config });
        self
    }

    /// Whether [`Self::admit_rate_limit`] would charge a bucket for `op`.
    ///
    /// Pure, and separate from `admit_rate_limit`, because deriving a
    /// bucket key is not free for a transport and can itself refuse the
    /// request: HTTP's default derivation answers `412` when the caller
    /// has no verifiable identity (cratestack#416). An exempt op must not
    /// pay that — cratestack#474 made a `@no_rate_limit` route reachable
    /// by callers with no identity at all — so a transport asks this first
    /// and derives a key only when the answer is yes.
    ///
    /// An op the caller could not identify arrives as
    /// [`OpAdmission::unresolved`], whose `rate_limited_by_default` is
    /// `true`, so it is charged. That is rate limiting's fail-closed
    /// direction ("when in doubt, apply the protection"), and it is the
    /// opposite polarity from idempotency's, where the same doubt reserves.
    pub fn rate_limit_applies(&self, op: &OpAdmission) -> bool {
        self.rate_limit.is_some() && op.rate_limited_by_default
    }

    /// Decide whether this call is charged, and charge it.
    ///
    /// A store error is returned as-is: what a *transport* does with an
    /// unreachable limiter (serve through it, or refuse) is cratestack#846's
    /// [`crate::StoreErrorPolicy`], which the transport applies because it
    /// owns the log line and the response.
    ///
    /// An op that [`Self::rate_limit_applies`] to but whose input carries
    /// no [`OpInput::rate_limit_bucket`] is refused with
    /// `CratestackError::Internal` rather than admitted: every
    /// participating call is charged to *some* bucket, and a caller that
    /// forgot to say which one has a bug that must not read as an
    /// exemption.
    pub async fn admit_rate_limit(
        &self,
        input: &OpInput<'_>,
    ) -> Result<RateLimitAdmission, CratestackError> {
        let Some(limiter) = self.rate_limit.as_ref() else {
            return Ok(RateLimitAdmission::Bypass);
        };
        if !input.op.rate_limited_by_default {
            return Ok(RateLimitAdmission::Bypass);
        }
        let Some(bucket) = input.rate_limit_bucket else {
            return Err(CratestackError::Internal(
                "rate limit: a rate-limited op was admitted without a bucket key; refusing \
                 rather than exempting it"
                    .to_owned(),
            ));
        };

        let request = ConsumeRequest::new(bucket.key, limiter.config, bucket.budget);
        let outcome = limiter.store.consume_bounded(request).await?;
        Ok(RateLimitAdmission::Consumed(outcome))
    }
}

impl<'a> OpInput<'a> {
    /// An input for a caller that asks only the rate-limit question.
    ///
    /// It carries no idempotency key, so handing it to
    /// [`OpExecutor::admit`] answers [`crate::Admission::Bypass`] — a
    /// rate-limit input can never take an idempotency reservation by
    /// accident. The principal and fingerprint are therefore unread, and
    /// set to empty rather than invented.
    pub fn for_rate_limit(op: OpAdmission, bucket: RateLimitBucket<'a>) -> Self {
        Self {
            op,
            principal: "",
            idempotency_key: None,
            fingerprint: [0; 32],
            ctx: None,
            rate_limit_bucket: Some(bucket),
        }
    }
}
