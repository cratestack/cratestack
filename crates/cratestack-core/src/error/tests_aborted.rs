//! GHSA-r67q-4qqq-g9gm: `TransactionAborted`'s code, and which of its
//! ownership states an idempotency layer may release.

use super::*;

fn info() -> DbErrorInfo {
    DbErrorInfo {
        detail: "transaction could not be completed; retry the request".to_owned(),
        sqlstate: Some("40001".to_owned()),
        constraint: None,
    }
}

/// Every state is reached the only way code outside this crate can reach
/// it: `Claimed` through the generated dispatch's claim of an exhausted one.
fn aborted(ownership: AbortOwnership) -> CratestackError {
    let error = match ownership {
        AbortOwnership::Exhausted | AbortOwnership::Claimed => {
            CratestackError::TransactionAborted(TransactionAbort::exhausted(info()))
        }
        AbortOwnership::Propagated => {
            CratestackError::TransactionAborted(TransactionAbort::propagated(info()))
        }
    };
    let error = if ownership == AbortOwnership::Claimed {
        error.__generated_claim_transaction_abort()
    } else {
        error
    };
    let CratestackError::TransactionAborted(abort) = &error else {
        unreachable!()
    };
    assert_eq!(abort.ownership(), ownership);
    error
}

/// An `@isolation` procedure out of retries is a 409 with its own code, so a
/// client can tell "send it again" from a unique violation's `CONFLICT`
/// without reading the message.
#[test]
fn transaction_aborted_is_a_409_with_its_own_code() {
    let err = aborted(AbortOwnership::Claimed);
    assert_eq!(err.status_code(), StatusCode::CONFLICT);
    assert_eq!(err.code(), "TRANSACTION_ABORTED");
    assert_ne!(err.code(), CratestackError::Conflict(String::new()).code());
    assert_eq!(err.db_sqlstate(), Some("40001"));
    assert!(CratestackError::Conflict("dup".to_owned()).is_idempotency_replayable());
    let response = err.into_response();
    assert_eq!(
        response.message,
        "transaction could not be completed; retry the request"
    );
}

/// Only the answering dispatch's own abort is released. One a caller
/// propagated out of its body, or one nobody claimed (a hand-written caller
/// of `invoke_with_db`), is recorded: that caller may have committed work.
#[test]
fn only_a_claimed_abort_is_not_replayable() {
    assert!(!aborted(AbortOwnership::Claimed).is_idempotency_replayable());
    assert!(aborted(AbortOwnership::Exhausted).is_idempotency_replayable());
    assert!(aborted(AbortOwnership::Propagated).is_idempotency_replayable());
}

/// The dispatch claims only an abort still `Exhausted`; a `Propagated` one
/// stays propagated, and propagation is sticky whatever the state was.
#[test]
fn claiming_and_propagating() {
    let claimed = aborted(AbortOwnership::Exhausted).__generated_claim_transaction_abort();
    assert!(!claimed.is_idempotency_replayable());
    let propagated = aborted(AbortOwnership::Exhausted).propagate_transaction_abort();
    assert!(
        propagated
            .__generated_claim_transaction_abort()
            .is_idempotency_replayable()
    );
    assert!(
        aborted(AbortOwnership::Claimed)
            .propagate_transaction_abort()
            .is_idempotency_replayable()
    );
    let other = CratestackError::Validation("x".to_owned()).__generated_claim_transaction_abort();
    assert!(matches!(other, CratestackError::Validation(_)));
}

/// Only the owner's abort is answered as `TRANSACTION_ABORTED`. Any other is
/// answered as a 500 `INTERNAL_ERROR` whose operator detail keeps the
/// original, and is replayable (recorded); every other error has no
/// substitute.
#[test]
fn only_a_claimed_abort_is_answered_as_transaction_aborted() {
    assert!(
        aborted(AbortOwnership::Claimed)
            .disowned_transaction_abort()
            .is_none()
    );
    for ownership in [AbortOwnership::Exhausted, AbortOwnership::Propagated] {
        let answered = aborted(ownership)
            .disowned_transaction_abort()
            .expect("a non-owner's abort is answered as an internal error");
        assert_eq!(answered.code(), "INTERNAL_ERROR", "{ownership:?}");
        assert_eq!(answered.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(answered.public_message(), "internal error");
        assert!(answered.is_idempotency_replayable(), "{ownership:?}");
        let detail = answered.detail().unwrap_or_default();
        assert!(detail.contains("40001"), "{detail}");
        assert!(detail.contains("retry the request"), "{detail}");
    }
    assert!(
        CratestackError::Conflict("dup".to_owned())
            .disowned_transaction_abort()
            .is_none()
    );
}
