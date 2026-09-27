use cratestack_core::{DbErrorInfo, TransactionAbort};

use super::*;

fn typed(sqlstate: Option<&str>, detail: &str) -> CratestackError {
    CratestackError::DatabaseTyped(DbErrorInfo {
        detail: detail.to_owned(),
        sqlstate: sqlstate.map(ToOwned::to_owned),
        constraint: None,
    })
}

/// Only database errors are classified by their text. The application's
/// own errors, and validation errors that echo request data, are not a
/// serialization failure however they read.
#[test]
fn only_database_errors_are_matched_by_text() {
    for error in [
        CratestackError::Validation("insufficient funds: requested 40001".to_owned()),
        CratestackError::Validation("field 'memo' length 40001 exceeds maximum 100".to_owned()),
        CratestackError::BadRequest("could not serialize access".to_owned()),
        CratestackError::Internal("deadlock detected in my own lock manager".to_owned()),
        CratestackError::Conflict("order 40P01 already exists".to_owned()),
    ] {
        assert_eq!(retriable_sqlstate(&error), None, "{error:?}");
    }
    assert_eq!(
        retriable_sqlstate(&CratestackError::Database(
            "error returned from database: could not serialize access due to concurrent \
             update"
                .to_owned()
        )),
        Some(PG_SERIALIZATION_FAILURE_SQLSTATE)
    );
}

/// A typed SQLSTATE is authoritative. What Postgres returns when a body
/// echoes request data into a `RAISE` or a cast is a database error whose
/// text contains `40001`, under another SQLSTATE: never retried.
#[test]
fn a_typed_sqlstate_is_authoritative_over_the_text() {
    for error in [
        typed(Some("P0001"), "insufficient funds: requested 40001"),
        typed(
            Some("22P02"),
            "invalid input syntax for type bigint: \"40001x\"",
        ),
        typed(Some("23505"), "duplicate key value (id)=(40P01)"),
        typed(
            Some("XX999"),
            "could not serialize access due to read/write dependencies",
        ),
        // A typed error without a code is not identified as one either.
        typed(None, "could not serialize access; deadlock detected"),
    ] {
        assert_eq!(retriable_sqlstate(&error), None, "{error:?}");
    }
    assert_eq!(
        retriable_sqlstate(&typed(Some("40001"), "anything")),
        Some(PG_SERIALIZATION_FAILURE_SQLSTATE)
    );
    assert_eq!(
        retriable_sqlstate(&typed(Some("40P01"), "anything")),
        Some(PG_DEADLOCK_DETECTED_SQLSTATE)
    );
}

/// An exhausted retry loop's outcome is final, whatever it carries and
/// whoever owns it.
#[test]
fn transaction_aborted_is_never_retriable() {
    let info = || DbErrorInfo {
        detail: "could not serialize access; retry the request".to_owned(),
        sqlstate: Some("40001".to_owned()),
        constraint: None,
    };
    for error in [
        CratestackError::TransactionAborted(TransactionAbort::exhausted(info())),
        CratestackError::TransactionAborted(TransactionAbort::propagated(info())),
        CratestackError::TransactionAborted(TransactionAbort::exhausted(info()))
            .__generated_claim_transaction_abort(),
    ] {
        let CratestackError::TransactionAborted(abort) = &error else {
            unreachable!()
        };
        let ownership = abort.ownership();
        assert_eq!(error.db_sqlstate(), Some("40001"));
        assert_eq!(retriable_sqlstate(&error), None, "{ownership:?}");
    }
}
