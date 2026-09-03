//! Bounding how many *distinct* rate-limit buckets one scope may create
//! (cratestack#871).
//!
//! # Why this is a store-side concept and not a middleware one
//!
//! The middleware knows *which* scope a request belongs to; only the store
//! knows how many buckets that scope has already minted, and only the
//! store can decide-and-charge atomically. Deciding in the middleware
//! would need a second round-trip and would race: N concurrent requests
//! each read "under budget" and each mint a bucket, which is precisely the
//! amplification being closed.
//!
//! The measured attack (security review of cratestack#846, quoted in that
//! PR's §6): `RateLimitLayer` runs *before* authentication, so its default
//! key derivation hashes an **unvalidated** `Authorization` header. A
//! caller rotating that header mints one store key per request, each with
//! a ≥60s TTL — 20 requests produced 20 buckets, and driving a real Redis
//! to `maxmemory` made every subsequent write fail.

use std::time::Duration;

use super::{RateLimitConfig, RateLimitDecision};

/// A cap on the number of distinct bucket keys one scope may create in a
/// window, plus where traffic beyond the cap is charged instead.
///
/// Deliberately carries the fallback bucket rather than an "and then
/// refuse" flag: refusing past the cap would hand an attacker a
/// *deterministic, global* outage of every rate-limited route — the exact
/// failure mode cratestack#846 was fought over. Collapsing onto the
/// fallback throttles the attacker (they now share one bucket) without
/// giving them a lever over anyone else's availability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BucketBudget {
    /// Identifies the set of buckets being counted — e.g. one verified
    /// peer address. Not itself a bucket key; stores namespace it
    /// separately.
    pub scope_key: String,
    /// Bucket charged once the scope is at its cap. Chosen by the caller
    /// so that it is a bucket the same caller would have used anyway (the
    /// peer's own `ip:` bucket), which is what makes the collapse a
    /// throttle rather than a bypass.
    pub fallback_key: String,
    /// How many distinct bucket keys this scope may admit while it lives.
    pub max_distinct: u32,
    /// **Floor** on how long the scope's admission record lives; the store
    /// raises it to at least `bucket_ttl_secs(config)` (cratestack#871
    /// review, blocker 2).
    ///
    /// It is a floor rather than a fixed window because a scope that
    /// expired *before* the buckets it admitted did not bound anything: a
    /// fresh scope re-admits `max_distinct` more while the previous
    /// generation is still alive, so the real steady state was
    /// `max_distinct × ceil(bucket_ttl / window)` — measured at 21 buckets
    /// for a cap of 4 over five 1s windows, and ~184 320 per peer for a
    /// non-refilling bucket under the defaults. Tying the lifetime to the
    /// buckets' own TTL makes the scope outlive everything it admitted, so
    /// the steady-state bound is `max_distinct + 2` per scope.
    ///
    /// The lifetime is refreshed on every admission, so a scope stays
    /// alive while it is actively admitting and expires that long after it
    /// stops. A member admitted once stays admitted until the whole record
    /// expires; further distinct keys take the fallback until then.
    ///
    /// A transient overlap of up to `2 × max_distinct` is still reachable:
    /// a bucket touched shortly before its scope expires can outlive it by
    /// up to one bucket TTL while a new generation fills. It rejoins the
    /// new scope's count on its next request, so it does not compound.
    pub window: Duration,
}

impl BucketBudget {
    pub fn new(
        scope_key: impl Into<String>,
        fallback_key: impl Into<String>,
        max_distinct: u32,
        window: Duration,
    ) -> Self {
        Self {
            scope_key: scope_key.into(),
            fallback_key: fallback_key.into(),
            max_distinct,
            window,
        }
    }
}

/// One token-consumption request: the bucket the caller *asked* for, the
/// token-bucket parameters, and optionally the budget that governs whether
/// that bucket may be created at all.
///
/// A struct rather than three positional arguments because
/// [`super::RateLimitStore::consume_bounded`] is a trait method that
/// out-of-tree stores implement: adding a field here is additive for them,
/// where adding a parameter is not.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct ConsumeRequest<'a> {
    pub key: &'a str,
    pub config: RateLimitConfig,
    pub budget: Option<&'a BucketBudget>,
}

impl<'a> ConsumeRequest<'a> {
    pub fn new(key: &'a str, config: RateLimitConfig, budget: Option<&'a BucketBudget>) -> Self {
        Self {
            key,
            config,
            budget,
        }
    }

    /// The bucket actually charged for this request, given a decision the
    /// store already made about the budget.
    pub fn charged_key(&self, charged: Charged) -> &'a str {
        match (charged, self.budget) {
            (Charged::Fallback | Charged::Overflow, Some(budget)) => &budget.fallback_key,
            _ => self.key,
        }
    }
}

/// Which bucket a [`super::RateLimitStore::consume_bounded`] call ended up
/// charging, and why. Purely observational — the decision itself is
/// carried by [`BoundedOutcome::decision`] — but it is what lets the
/// middleware log an in-progress amplification attempt instead of silently
/// absorbing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Charged {
    /// The requested bucket, either already known to the scope or newly
    /// admitted under the cap. The normal case, and the one that preserves
    /// cratestack#416: distinct callers under the budget never share.
    Requested,
    /// The scope was at its cap, so the fallback bucket was charged
    /// instead. Stores report this for every over-cap charge; the
    /// middleware refines it to [`Charged::Overflow`] when it knows the
    /// scope was the process-global one (a store cannot tell the two
    /// apart, and giving it a way to would duplicate that rule in every
    /// backend).
    Fallback,
    /// As [`Charged::Fallback`], but the scope was the global one used
    /// when no verified peer address is available at all — i.e. the
    /// deployment is not wired through
    /// `into_make_service_with_connect_info` AND is under an amplification
    /// attempt. Distinguished because it is the only case where unrelated
    /// callers can be collapsed together, so it warrants a louder log.
    Overflow,
    /// The store does not implement `consume_bounded`, so no bound was
    /// applied. Emitted by the trait's default implementation.
    Unbounded,
}

/// What [`super::RateLimitStore::consume_bounded`] returns: the ordinary
/// decision, plus which bucket produced it.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct BoundedOutcome {
    pub decision: RateLimitDecision,
    pub charged: Charged,
}

impl BoundedOutcome {
    pub fn new(decision: RateLimitDecision, charged: Charged) -> Self {
        Self { decision, charged }
    }
}
