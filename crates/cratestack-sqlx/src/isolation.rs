//! Helpers for running banking-grade multi-row mutations under explicit
//! transaction isolation, with retry on serialization failure.
//!
//! Procedures opt in via `@isolation("serializable")` in the schema; the
//! macro records the requested level on a `ProcedureMetadata` const and
//! handler code can wrap its body in [`run_in_isolated_tx`] to actually
//! enforce it. A follow-up will auto-wrap procedure dispatch so opting in
//! requires only the attribute.
use crate::sqlx;

use std::future::Future;

use cratestack_core::{CoolError, TransactionIsolation};

use crate::error::cool_error_from_sqlx;

const MAX_RETRIES_DEFAULT: u32 = 3;
const PG_SERIALIZATION_FAILURE_SQLSTATE: &str = "40001";
const PG_DEADLOCK_DETECTED_SQLSTATE: &str = "40P01";

/// Begin a transaction at the requested isolation level, run `body` against
/// the live transaction, and commit. On `40001` (serialization_failure) or
/// `40P01` (deadlock_detected) the transaction is rolled back and the body
/// runs again, up to `MAX_RETRIES_DEFAULT` times. Other errors propagate
/// immediately.
///
/// `body` receives a mutable transaction reference; it should run all of
/// its SQL through that reference so the writes participate in the same
/// transaction.
pub async fn run_in_isolated_tx<F, Fut, T>(
    pool: &sqlx::PgPool,
    isolation: TransactionIsolation,
    body: F,
) -> Result<T, CoolError>
where
    F: FnMut(sqlx::Transaction<'static, sqlx::Postgres>) -> Fut,
    Fut: Future<Output = Result<(T, sqlx::Transaction<'static, sqlx::Postgres>), CoolError>>,
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
) -> Result<T, CoolError>
where
    F: FnMut(sqlx::Transaction<'static, sqlx::Postgres>) -> Fut,
    Fut: Future<Output = Result<(T, sqlx::Transaction<'static, sqlx::Postgres>), CoolError>>,
{
    let mut attempts = 0u32;
    loop {
        attempts += 1;
        let mut tx = pool.begin().await.map_err(cool_error_from_sqlx)?;
        let set_stmt = format!("SET TRANSACTION ISOLATION LEVEL {}", isolation.as_sql());
        sqlx::query(&set_stmt)
            .execute(&mut *tx)
            .await
            .map_err(cool_error_from_sqlx)?;

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
                    let promoted = cool_error_from_sqlx(commit_error);
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

fn is_retriable(error: &CoolError) -> bool {
    // Fast path: typed variant surfaces the SQLSTATE directly.
    if let Some(code) = error.db_sqlstate() {
        return code == PG_SERIALIZATION_FAILURE_SQLSTATE || code == PG_DEADLOCK_DETECTED_SQLSTATE;
    }
    // Fallback: legacy `Database(String)` variant — substring-match the detail
    // string the way the original code did, so existing behaviour is preserved.
    let detail = error.detail().unwrap_or_default();
    detail.contains(PG_SERIALIZATION_FAILURE_SQLSTATE)
        || detail.contains(PG_DEADLOCK_DETECTED_SQLSTATE)
        || detail.contains("could not serialize access")
        || detail.contains("deadlock detected")
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
        let err = CoolError::Database(
            "Database(PgDatabaseError { severity: ERROR, code: \"40001\", \
             message: \"could not serialize access due to concurrent update\" })"
                .to_owned(),
        );
        assert!(is_retriable(&err));
    }

    #[test]
    fn retriable_on_deadlock_sqlstate() {
        let err = CoolError::Database(
            "Database(PgDatabaseError { code: \"40P01\", \
             message: \"deadlock detected\" })"
                .to_owned(),
        );
        assert!(is_retriable(&err));
    }

    #[test]
    fn not_retriable_on_unique_violation() {
        let err = CoolError::Database(
            "duplicate key value violates unique constraint \"accounts_pkey\"".to_owned(),
        );
        assert!(!is_retriable(&err));
    }

    #[test]
    fn retriable_when_serialization_failure_is_raised_at_commit_time() {
        // PG SSI can defer the 40001 to COMMIT. The sqlx error surfaced
        // by `tx.commit()` carries the same SQLSTATE; the loop now
        // promotes that into `CoolError::Database` and feeds it through
        // `is_retriable` so the commit-time path is no longer leaked to
        // callers despite the API advertising automatic retries.
        let err = CoolError::Database(
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
        let err = CoolError::DatabaseTyped(DbErrorInfo {
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
        let err = CoolError::DatabaseTyped(DbErrorInfo {
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
        let err = CoolError::DatabaseTyped(DbErrorInfo {
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
    fn typed_variant_exposes_constraint_for_unique_violation() {
        use cratestack_core::DbErrorInfo;
        let err = CoolError::DatabaseTyped(DbErrorInfo {
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
