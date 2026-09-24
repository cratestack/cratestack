//! Rate-limit admission's failure directions (slice 2, cratestack#877):
//! what happens when the op is unidentified, the bucket is missing, or the
//! store fails — and that a rate-limit input cannot leak into idempotency.

use std::sync::Arc;
use std::time::Duration;

use cratestack_core::{BoundedOutcome, CratestackError, RateLimitDecision};

use super::tests_rate_limit::{RecordingStore, bucket_input, limited, participating};
use crate::{Admission, OpAdmission, OpExecutor, OpInput, RateLimitAdmission};

/// FAIL DIRECTION, rate limiting: an op the transport could not identify is
/// CHARGED. For rate limiting "when in doubt, apply the protection" means
/// throttle it; a miss that bypassed would let any unmatched path — a typo,
/// a schema/router mismatch — spend nothing. This is the opposite polarity
/// from idempotency, where the same `OpAdmission::unresolved()` RESERVES
/// (asserted in `tests_admission`).
#[tokio::test]
async fn an_unidentified_op_is_charged_not_exempted() {
    let store = RecordingStore::answering(RateLimitDecision::Throttled {
        retry_after_secs: 3,
    });
    let executor = limited(&store);

    assert!(executor.rate_limit_applies(&OpAdmission::unresolved()));
    let admission = executor
        .admit_rate_limit(&bucket_input(OpAdmission::unresolved()))
        .await
        .expect("the fake store answers");
    assert!(matches!(
        admission,
        RateLimitAdmission::Consumed(BoundedOutcome {
            decision: RateLimitDecision::Throttled { .. },
            ..
        })
    ));
    assert_eq!(store.calls().len(), 1);
}

/// A participating op with no bucket is a caller bug. Refusing is the only
/// answer that cannot read as an exemption.
#[tokio::test]
async fn a_participating_op_without_a_bucket_is_refused_not_exempted() {
    let store = RecordingStore::answering(RateLimitDecision::Allowed { remaining: 6 });
    let executor = limited(&store);
    let input = OpInput::new(participating(), "principal", None, [0; 32]);

    let error = executor
        .admit_rate_limit(&input)
        .await
        .err()
        .expect("a missing bucket must not admit");
    assert!(matches!(error, CratestackError::Internal(_)));
    assert!(store.calls().is_empty());
}

/// Store failures are the transport's to classify (cratestack#846's
/// `StoreErrorPolicy`), so the executor must hand the error back, not
/// decide it — swallowing it here would bypass `Deny`.
#[tokio::test]
async fn a_store_error_is_returned_for_the_transport_to_classify() {
    let store = RecordingStore::unavailable("redis down");
    let executor = limited(&store);

    let error = executor
        .admit_rate_limit(&bucket_input(participating()))
        .await
        .err()
        .expect("the store failed");
    assert!(matches!(error, CratestackError::Unavailable(_)));
}

/// An input built for rate limiting carries no idempotency key, so the
/// idempotency question answers `Bypass` even on an executor that has an
/// idempotency store: a rate-limit input cannot take a reservation.
#[tokio::test]
async fn a_rate_limit_input_cannot_take_an_idempotency_reservation() {
    let idempotency = Arc::new(super::tests_admission::CountingStore::reserving());
    let executor = OpExecutor::new(Some(idempotency.clone()), Duration::from_secs(60));

    let admission = executor
        .admit(&bucket_input(participating()))
        .await
        .expect("a bypass cannot fail");
    assert!(matches!(admission, Admission::Bypass));
    assert_eq!(
        idempotency
            .reserve_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}
