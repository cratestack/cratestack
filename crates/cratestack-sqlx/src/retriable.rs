//! Which database errors mean "run the whole transaction again"
//! (docs/design/procedure-isolation.md §5): the `@isolation` dispatch's
//! attempt loop and the hand-rolled [`crate::run_in_isolated_tx`] both
//! classify through [`retriable_sqlstate`].
//!
//! Three decisions, each closing a request-triggerable re-run of a body:
//!
//! - **Only database errors.** An application or validation error echoing
//!   `40001` (`field 'memo' length 40001 exceeds maximum 100`) is not a
//!   serialization failure. The hand-rolled helper used to text-match every
//!   variant; it was aligned because that is a correctness bug on any path.
//! - **A typed SQLSTATE is authoritative.** A database error that carries a
//!   SQLSTATE is retriable iff that SQLSTATE is `40001` or `40P01`; its text
//!   is never consulted. Postgres echoes request data into the messages of
//!   other SQLSTATEs — `RAISE EXCEPTION 'insufficient funds: requested %'`
//!   (`P0001`), `invalid input syntax for type bigint: "40001x"` (`22P02`) —
//!   and matching that text re-ran the body, answered `409
//!   TRANSACTION_ABORTED` and released the `Idempotency-Key`. Text matching
//!   remains only for the untyped `Database(String)` variant, which has no
//!   SQLSTATE to read (non-`Database` sqlx errors, hand-built errors). The
//!   crate's own read, policy and audit paths build their errors with
//!   `cratestack_error_from_sqlx`, so a Postgres error from them is typed.
//! - **`TransactionAborted` is final.** It carries the `40001` its retries
//!   ran out on, but it is the outcome of a retry loop, not a statement's
//!   failure: an outer loop it propagates into must not run again.

use cratestack_core::CratestackError;

pub(crate) const PG_SERIALIZATION_FAILURE_SQLSTATE: &str = "40001";
pub(crate) const PG_DEADLOCK_DETECTED_SQLSTATE: &str = "40P01";

/// `Some("40001")` for a serialization failure, `Some("40P01")` for a
/// detected deadlock, `None` for anything else.
pub(crate) fn retriable_sqlstate(error: &CratestackError) -> Option<&'static str> {
    match error {
        CratestackError::DatabaseTyped(info) | CratestackError::ConflictTyped(info) => {
            retriable_code(info.sqlstate.as_deref()?)
        }
        CratestackError::Database(detail) => retriable_text(detail),
        // `TransactionAborted` is deliberately here: final, never retried.
        _ => None,
    }
}

fn retriable_code(sqlstate: &str) -> Option<&'static str> {
    match sqlstate {
        PG_SERIALIZATION_FAILURE_SQLSTATE => Some(PG_SERIALIZATION_FAILURE_SQLSTATE),
        PG_DEADLOCK_DETECTED_SQLSTATE => Some(PG_DEADLOCK_DETECTED_SQLSTATE),
        _ => None,
    }
}

/// The legacy untyped variant: substring-match the driver's text, the way
/// the original code did. There is no SQLSTATE to prefer.
fn retriable_text(detail: &str) -> Option<&'static str> {
    if detail.contains(PG_SERIALIZATION_FAILURE_SQLSTATE)
        || detail.contains("could not serialize access")
    {
        return Some(PG_SERIALIZATION_FAILURE_SQLSTATE);
    }
    if detail.contains(PG_DEADLOCK_DETECTED_SQLSTATE) || detail.contains("deadlock detected") {
        return Some(PG_DEADLOCK_DETECTED_SQLSTATE);
    }
    None
}

#[cfg(test)]
mod tests;
