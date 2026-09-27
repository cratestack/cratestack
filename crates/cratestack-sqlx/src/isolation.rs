//! Helpers for running banking-grade multi-row mutations under explicit
//! transaction isolation, with retry on serialization failure.
//!
//! These are the hand-rolled form, for code that owns a pool. A
//! *procedure* declaring `@isolation("...")` in the schema does not need
//! them: its generated dispatch runs the procedure's authorization and
//! body inside a transaction at the declared level, with its own retry
//! loop, on every transport (REST, RPC, RPC batch, MCP and
//! `invoke_with_db`) — see `SqlxRuntime::run_isolated` and
//! docs/design/procedure-isolation.md. Before GHSA-r67q-4qqq-g9gm was
//! fixed, this comment claimed a `ProcedureMetadata` constant recorded
//! the level for dispatch to use; no such constant ever existed and the
//! attribute had no effect.
//!
//! Like `db.transaction(...)` (see `crate::transaction`'s module doc,
//! cratestack#534), composing write-builder `run_in_tx` calls inside
//! `body` here does not get you automatic `AuditSink` fan-out or `@@emit`
//! delivery either, for the identical reason: `body` returns to this
//! helper's own retry loop, not to a commit hook this crate owns, so
//! dispatch remains the caller's responsibility after the outer call
//! returns `Ok`.
use crate::sqlx;

use std::future::Future;

use cratestack_core::{CratestackError, TransactionIsolation};

use crate::error::cratestack_error_from_sqlx;

pub(crate) const MAX_RETRIES_DEFAULT: u32 = 3;

/// Begin a transaction at the requested isolation level, run `body` against
/// the live transaction, and commit. On `40001` (serialization_failure) or
/// `40P01` (deadlock_detected) the transaction is rolled back and the body
/// runs again, up to `MAX_RETRIES_DEFAULT` times. Other errors propagate
/// immediately. Only a database error is retried: an error `body` builds
/// itself (`Validation`, `Internal`, …) is returned as is even when its
/// text contains `40001` or `deadlock detected`, and a typed database error
/// is retried by its SQLSTATE alone, never by its text.
///
/// `body` receives a mutable transaction reference; it should run all of
/// its SQL through that reference so the writes participate in the same
/// transaction.
pub async fn run_in_isolated_tx<F, Fut, T>(
    pool: &sqlx::PgPool,
    isolation: TransactionIsolation,
    body: F,
) -> Result<T, CratestackError>
where
    F: FnMut(sqlx::Transaction<'static, sqlx::Postgres>) -> Fut,
    Fut: Future<Output = Result<(T, sqlx::Transaction<'static, sqlx::Postgres>), CratestackError>>,
{
    run_in_isolated_tx_with_retries(pool, isolation, MAX_RETRIES_DEFAULT, body).await
}

/// Same as [`run_in_isolated_tx`] but with a caller-chosen retry budget.
/// Banks running long-tail contended writes sometimes want a higher cap
/// (5–10); single-row CAS workflows can drop to 1 to fail fast.
pub async fn run_in_isolated_tx_with_retries<F, Fut, T>(
    pool: &sqlx::PgPool,
    isolation: TransactionIsolation,
    max_retries: u32,
    mut body: F,
) -> Result<T, CratestackError>
where
    F: FnMut(sqlx::Transaction<'static, sqlx::Postgres>) -> Fut,
    Fut: Future<Output = Result<(T, sqlx::Transaction<'static, sqlx::Postgres>), CratestackError>>,
{
    let mut attempts = 0u32;
    loop {
        attempts += 1;
        let mut tx = pool.begin().await.map_err(cratestack_error_from_sqlx)?;
        let set_stmt = format!("SET TRANSACTION ISOLATION LEVEL {}", isolation.as_sql());
        // `AssertSqlSafe`: `isolation.as_sql()` returns one of four
        // `&'static str` literals, so nothing request-derived reaches this
        // statement (sqlx 0.9's `SqlSafeStr` bound).
        sqlx::query(sqlx::AssertSqlSafe(set_stmt))
            .execute(&mut *tx)
            .await
            .map_err(cratestack_error_from_sqlx)?;

        match body(tx).await {
            Ok((value, tx)) => match tx.commit().await {
                Ok(()) => return Ok(value),
                Err(commit_error) => {
                    // PG can defer a serialization anomaly all the way to
                    // COMMIT: the body's SQL runs cleanly, then the engine
                    // detects the conflict during the predicate-lock check
                    // at commit and rolls the transaction back with
                    // SQLSTATE 40001 (the docs are explicit that the
                    // *entire* transaction must be retried). Without this
                    // branch we'd advertise automatic retries but still
                    // leak a transient 40001 to callers when the conflict
                    // is detected at the commit boundary.
                    let promoted = cratestack_error_from_sqlx(commit_error);
                    if attempts <= max_retries && is_retriable(&promoted) {
                        tokio::task::yield_now().await;
                        continue;
                    }
                    return Err(promoted);
                }
            },
            Err(error) => {
                if attempts <= max_retries && is_retriable(&error) {
                    // Backoff is intentionally trivial — banks running this
                    // under heavy contention should swap to a more thoughtful
                    // jittered backoff. Sub-millisecond pause yields the
                    // current task without keeping a tx open.
                    tokio::task::yield_now().await;
                    continue;
                }
                return Err(error);
            }
        }
    }
}

/// The same classifier as an `@isolation` procedure's dispatch
/// ([`crate::retriable::retriable_sqlstate`]): the SQLSTATE of a typed
/// database error — authoritative, its text never consulted — else the
/// driver's text of the untyped `Database(String)` variant. Only database
/// errors: an application or validation error whose message happens to
/// contain `40001` (a body echoing request data) is not a serialization
/// failure, and neither is a typed `P0001`/`22P02` whose message echoes it;
/// retrying either re-ran `body` with its side effects and returned the
/// wrong error (docs/design/procedure-isolation.md §5).
fn is_retriable(error: &CratestackError) -> bool {
    crate::retriable::retriable_sqlstate(error).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_isolation_levels() {
        assert_eq!(
            TransactionIsolation::parse("serializable").unwrap(),
            TransactionIsolation::Serializable,
        );
        assert_eq!(
            TransactionIsolation::parse("Repeatable_Read").unwrap(),
            TransactionIsolation::RepeatableRead,
        );
        assert_eq!(
            TransactionIsolation::parse("read committed").unwrap(),
            TransactionIsolation::ReadCommitted,
        );
        assert!(TransactionIsolation::parse("snapshot").is_err());
    }

    #[test]
    fn sql_strings_match_pg_grammar() {
        assert_eq!(TransactionIsolation::Serializable.as_sql(), "SERIALIZABLE");
        assert_eq!(
            TransactionIsolation::RepeatableRead.as_sql(),
            "REPEATABLE READ",
        );
        assert_eq!(
            TransactionIsolation::ReadCommitted.as_sql(),
            "READ COMMITTED",
        );
    }

    #[test]
    fn retriable_on_serialization_failure_sqlstate() {
        let err = CratestackError::Database(
            "Database(PgDatabaseError { severity: ERROR, code: \"40001\", \
             message: \"could not serialize access due to concurrent update\" })"
                .to_owned(),
        );
        assert!(is_retriable(&err));
    }

    #[test]
    fn retriable_on_deadlock_sqlstate() {
        let err = CratestackError::Database(
            "Database(PgDatabaseError { code: \"40P01\", \
             message: \"deadlock detected\" })"
                .to_owned(),
        );
        assert!(is_retriable(&err));
    }

    #[test]
    fn not_retriable_on_an_application_error_that_mentions_40001() {
        for err in [
            CratestackError::Validation("field 'memo' length 40001 exceeds maximum 100".to_owned()),
            CratestackError::Internal("deadlock detected in my own lock manager".to_owned()),
        ] {
            assert!(!is_retriable(&err), "{err:?}");
        }
    }

    #[test]
    fn not_retriable_on_unique_violation() {
        let err = CratestackError::Database(
            "duplicate key value violates unique constraint \"accounts_pkey\"".to_owned(),
        );
        assert!(!is_retriable(&err));
    }

    #[test]
    fn retriable_when_serialization_failure_is_raised_at_commit_time() {
        // PG SSI can defer the 40001 to COMMIT. The sqlx error surfaced
        // by `tx.commit()` carries the same SQLSTATE; the loop now
        // promotes that into `CratestackError::Database` and feeds it through
        // `is_retriable` so the commit-time path is no longer leaked to
        // callers despite the API advertising automatic retries.
        let err = CratestackError::Database(
            "Database(PgDatabaseError { severity: ERROR, code: \"40001\", \
             message: \"could not serialize access due to read/write dependencies among transactions\" })"
                .to_owned(),
        );
        assert!(is_retriable(&err));
    }

    // --- typed-variant paths ---

    #[test]
    fn retriable_typed_serialization_failure() {
        use cratestack_core::DbErrorInfo;
        let err = CratestackError::DatabaseTyped(DbErrorInfo {
            detail: "could not serialize access due to concurrent update".to_owned(),
            sqlstate: Some("40001".to_owned()),
            constraint: None,
        });
        assert!(
            is_retriable(&err),
            "DatabaseTyped with 40001 sqlstate must be retriable via the fast path",
        );
    }

    #[test]
    fn retriable_typed_deadlock() {
        use cratestack_core::DbErrorInfo;
        let err = CratestackError::DatabaseTyped(DbErrorInfo {
            detail: "deadlock detected".to_owned(),
            sqlstate: Some("40P01".to_owned()),
            constraint: None,
        });
        assert!(
            is_retriable(&err),
            "DatabaseTyped with 40P01 sqlstate must be retriable via the fast path",
        );
    }

    #[test]
    fn not_retriable_typed_unique_violation() {
        use cratestack_core::DbErrorInfo;
        let err = CratestackError::DatabaseTyped(DbErrorInfo {
            detail: "duplicate key value violates unique constraint \"accounts_pkey\"".to_owned(),
            sqlstate: Some("23505".to_owned()),
            constraint: Some("accounts_pkey".to_owned()),
        });
        assert!(
            !is_retriable(&err),
            "unique_violation (23505) must not be retried",
        );
    }

    #[test]
    fn typed_variant_sqlstate_is_authoritative_over_its_detail() {
        // A typed error's SQLSTATE decides; its text is never consulted.
        // The text of other SQLSTATEs echoes request data: a body's
        // `RAISE EXCEPTION 'insufficient funds: requested %'` (P0001), a
        // cast of `'40001x'` (22P02). Matching it re-ran the body (and, in
        // `@isolation` dispatch, answered 409 and released the idempotency
        // key) — request-triggerable. An unknown SQLSTATE is not retried
        // either, whatever its detail says.
        use cratestack_core::DbErrorInfo;
        for (sqlstate, detail) in [
            (
                "XX999",
                "could not serialize access due to read/write dependencies",
            ),
            ("P0001", "insufficient funds: requested 40001"),
            ("22P02", "invalid input syntax for type bigint: \"40001x\""),
            ("55P03", "deadlock detected while waiting for 40P01"),
        ] {
            let err = CratestackError::DatabaseTyped(DbErrorInfo {
                detail: detail.to_owned(),
                sqlstate: Some(sqlstate.to_owned()),
                constraint: None,
            });
            assert!(
                !is_retriable(&err),
                "typed {sqlstate} must not be retried because of its text",
            );
        }
    }

    #[test]
    fn typed_variant_exposes_constraint_for_unique_violation() {
        use cratestack_core::DbErrorInfo;
        let err = CratestackError::DatabaseTyped(DbErrorInfo {
            detail: "duplicate key value violates unique constraint \"wallets_owner_key\""
                .to_owned(),
            sqlstate: Some("23505".to_owned()),
            constraint: Some("wallets_owner_key".to_owned()),
        });
        assert_eq!(err.db_sqlstate(), Some("23505"));
        assert_eq!(err.db_constraint(), Some("wallets_owner_key"));
        // Public message must remain canned — no detail leak.
        assert_eq!(err.public_message(), "internal error");
    }
}
