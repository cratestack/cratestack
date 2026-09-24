//! Rate-limit admission (slice 2, cratestack#877): which calls reach the
//! store, what they are charged against, and which way each doubt fails.
//!
//! The store is a recording fake: every assertion that something was NOT
//! charged is a call count of zero, which is the only way to tell "bypassed"
//! from "charged and allowed" — both let the call run. The fail-direction
//! half lives in `tests_rate_limit_fail`, sharing this fake rather than
//! re-declaring it.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use cratestack_core::{
    BoundedOutcome, BucketBudget, Charged, ConsumeRequest, CratestackError, RateLimitConfig,
    RateLimitDecision, RateLimitStore,
};

use crate::{OpAdmission, OpExecutor, OpInput, RateLimitAdmission, RateLimitBucket};

/// What one `consume_bounded` call was asked, recorded owned.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Seen {
    key: String,
    burst: u32,
    budget: Option<BucketBudget>,
}

pub(crate) struct RecordingStore {
    /// `Err` holds the message of a `CratestackError::Unavailable`, rebuilt
    /// per call because `CratestackError` is not `Clone`.
    answer: Result<BoundedOutcome, String>,
    seen: Mutex<Vec<Seen>>,
}

impl RecordingStore {
    pub(crate) fn answering(decision: RateLimitDecision) -> Arc<Self> {
        Arc::new(Self {
            answer: Ok(BoundedOutcome::new(decision, Charged::Requested)),
            seen: Mutex::new(Vec::new()),
        })
    }

    pub(crate) fn unavailable(message: &str) -> Arc<Self> {
        Arc::new(Self {
            answer: Err(message.to_owned()),
            seen: Mutex::new(Vec::new()),
        })
    }

    pub(crate) fn calls(&self) -> Vec<Seen> {
        self.seen
            .lock()
            .map(|seen| seen.clone())
            .unwrap_or_default()
    }
}

#[async_trait]
impl RateLimitStore for RecordingStore {
    async fn consume(
        &self,
        _key: &str,
        _config: RateLimitConfig,
    ) -> Result<RateLimitDecision, CratestackError> {
        Err(CratestackError::Internal(
            "the executor must call consume_bounded, which carries the budget".to_owned(),
        ))
    }

    async fn consume_bounded(
        &self,
        request: ConsumeRequest<'_>,
    ) -> Result<BoundedOutcome, CratestackError> {
        if let Ok(mut seen) = self.seen.lock() {
            seen.push(Seen {
                key: request.key.to_owned(),
                burst: request.config.burst,
                budget: request.budget.cloned(),
            });
        }
        self.answer.clone().map_err(CratestackError::Unavailable)
    }
}

const CONFIG: RateLimitConfig = RateLimitConfig {
    burst: 7,
    refill_per_second: 1.0,
};

pub(crate) fn limited(store: &Arc<RecordingStore>) -> OpExecutor {
    OpExecutor::new(None, Duration::ZERO)
        .with_rate_limit(store.clone() as Arc<dyn RateLimitStore>, CONFIG)
}

fn exempt() -> OpAdmission {
    OpAdmission::new("procedure.health", true, false)
}

pub(crate) fn participating() -> OpAdmission {
    OpAdmission::new("procedure.transfer", false, true)
}

pub(crate) fn bucket_input(op: OpAdmission) -> OpInput<'static> {
    OpInput::for_rate_limit(op, RateLimitBucket::new("ip:192.0.2.1", None))
}

#[tokio::test]
async fn no_limiter_wired_bypasses_and_applies_to_nothing() {
    let executor = OpExecutor::new(None, Duration::ZERO);

    assert!(!executor.rate_limit_applies(&participating()));
    assert!(!executor.rate_limit_applies(&OpAdmission::unresolved()));
    let admission = executor
        .admit_rate_limit(&bucket_input(participating()))
        .await
        .expect("a bypass cannot fail");
    assert!(matches!(admission, RateLimitAdmission::Bypass));
}

/// `@no_rate_limit` (cratestack#474): the store is never consulted, so the
/// exempt op spends nobody's budget — not just "allowed this time".
#[tokio::test]
async fn an_exempt_op_is_never_charged() {
    let store = RecordingStore::answering(RateLimitDecision::Allowed { remaining: 6 });
    let executor = limited(&store);

    assert!(!executor.rate_limit_applies(&exempt()));
    let admission = executor
        .admit_rate_limit(&bucket_input(exempt()))
        .await
        .expect("a bypass cannot fail");
    assert!(matches!(admission, RateLimitAdmission::Bypass));
    assert!(store.calls().is_empty(), "exempt op reached the store");
}

/// The store's answer comes back untouched — both halves of it — because
/// the transport renders `remaining`, `retry_after_secs` and the charged
/// bucket exactly as it did before this crate existed.
#[tokio::test]
async fn a_participating_op_is_charged_and_the_answer_passes_through() {
    for decision in [
        RateLimitDecision::Allowed { remaining: 6 },
        RateLimitDecision::Throttled {
            retry_after_secs: 9,
        },
    ] {
        let store = RecordingStore::answering(decision);
        let executor = limited(&store);

        assert!(executor.rate_limit_applies(&participating()));
        let admission = executor
            .admit_rate_limit(&bucket_input(participating()))
            .await
            .expect("the fake store answers");
        let RateLimitAdmission::Consumed(outcome) = admission else {
            panic!("a participating op must be charged, got a bypass");
        };
        assert_eq!(outcome.decision, decision);
        assert_eq!(outcome.charged, Charged::Requested);
        assert_eq!(
            store.calls(),
            vec![Seen {
                key: "ip:192.0.2.1".to_owned(),
                burst: 7,
                budget: None,
            }],
            "charged exactly once, against the caller's key, with the executor's config"
        );
    }
}

/// The cratestack#871 budget is the caller's to derive and the store's to
/// enforce; the executor only carries it across. Dropping it here would
/// silently unbound the keyspace again.
#[tokio::test]
async fn the_bucket_budget_reaches_the_store() {
    let store = RecordingStore::answering(RateLimitDecision::Allowed { remaining: 6 });
    let executor = limited(&store);
    let budget = BucketBudget::new(
        "peer:192.0.2.1",
        "ip:192.0.2.1",
        128,
        Duration::from_secs(60),
    );
    let input = OpInput::for_rate_limit(
        participating(),
        RateLimitBucket::new("auth:abc", Some(&budget)),
    );

    executor
        .admit_rate_limit(&input)
        .await
        .expect("the fake store answers");

    let calls = store.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].key, "auth:abc");
    assert_eq!(calls[0].budget.as_ref(), Some(&budget));
}
