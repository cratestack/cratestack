//! [`SqlxRuntime::run_isolated`]: the retry loop an `@isolation`
//! procedure's generated `invoke_with_db` runs its authorization and body
//! in (docs/design/procedure-isolation.md §5, GHSA-r67q-4qqq-g9gm).

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use cratestack_core::{CratestackError, DbErrorInfo, TransactionAbort, TransactionIsolation};

use crate::audit::dispatch_audit_sink;
use crate::bound::BoundTx;
use crate::descriptor::SqlxRuntime;
use crate::error::cratestack_error_from_sqlx;
use crate::retriable::retriable_sqlstate;
use crate::sqlx;
use crate::transaction::Tx;

/// Public detail of the error an exhausted retry budget produces.
const ISOLATION_CONFLICT_DETAIL: &str =
    "transaction could not be completed because of concurrent updates; retry the request";

enum Attempt<T> {
    Committed(T),
    Retry(&'static str),
    Failed(CratestackError),
}

impl SqlxRuntime {
    /// The transaction this runtime is bound to, when it is the
    /// per-attempt runtime of an `@isolation` procedure.
    pub(crate) fn bound(&self) -> Option<&BoundTx> {
        self.bound.as_deref()
    }

    /// Retries an `@isolation` procedure gets after a serialization
    /// failure (`40001`) or detected deadlock (`40P01`) before the
    /// request fails with `409 TRANSACTION_ABORTED`. Default 3; `0`
    /// disables retry.
    pub fn with_isolation_max_retries(mut self, max_retries: u32) -> Self {
        self.isolation_max_retries = max_retries;
        self
    }

    /// Run `body` inside one transaction begun at `isolation`, committing
    /// on `Ok`. `body` receives (by value, so the future it returns can own
    /// it) a runtime bound to that transaction: every
    /// builder, `transaction()` and `@authorize` probe made through it
    /// runs on the transaction. On `40001`/`40P01` — from a statement,
    /// from `COMMIT`, or observed by any operation during the attempt even
    /// if `body` swallowed it — the attempt is rolled back and `body` runs
    /// again after a short backoff, up to the configured retry budget,
    /// then fails with `TransactionAborted` (409 `TRANSACTION_ABORTED`).
    /// `AuditSink` fan-out and the
    /// outbox drain the attempt's writes requested happen once, after the
    /// committed attempt.
    ///
    /// A `TransactionAborted` returned *by `body`* — another transaction's
    /// exhausted retries, propagated — is not retried (it is final) and is
    /// marked [`AbortOwnership::Propagated`](cratestack_core::AbortOwnership)
    /// so no dispatch can claim it as its own; only this loop's own
    /// exhaustion is returned as `Exhausted`.
    ///
    /// On a runtime that is already bound to an attempt (a nested
    /// `@isolation` call), `body` runs once as a savepoint of that attempt
    /// instead of in a new transaction, and the outermost attempt owns the
    /// retries (`bound::join_bound`, docs/design/procedure-isolation.md
    /// §7.1).
    ///
    /// Generated code calls this; it is public only so the facade crates
    /// can reach it.
    #[doc(hidden)]
    pub async fn run_isolated<F, Fut, T>(
        &self,
        isolation: TransactionIsolation,
        mut body: F,
    ) -> Result<T, CratestackError>
    where
        F: FnMut(SqlxRuntime) -> Fut,
        Fut: Future<Output = Result<T, CratestackError>>,
    {
        if let Some(bound) = self.bound() {
            return crate::bound::join_bound(self, bound, isolation, body).await;
        }
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            let begin = format!("BEGIN ISOLATION LEVEL {}", isolation.as_sql());
            // `AssertSqlSafe`: `as_sql()` is one of three `&'static str`
            // literals; nothing request-derived reaches this statement.
            let tx = self
                .pool()
                .begin_with(sqlx::AssertSqlSafe(begin))
                .await
                .map_err(cratestack_error_from_sqlx)?;
            let bound = Arc::new(BoundTx::new(Tx::new(tx), isolation));
            let mut runtime = self.clone();
            runtime.bound = Some(bound.clone());
            let result = body(runtime)
                .await
                .map_err(CratestackError::propagate_transaction_abort);
            match finish_attempt(&bound, result).await {
                Attempt::Committed(value) => {
                    let (events, drain) = bound.take_deferred();
                    if drain {
                        let _ = self.drain_event_outbox().await;
                    }
                    dispatch_audit_sink(self, &events).await;
                    return Ok(value);
                }
                Attempt::Retry(sqlstate) if attempt <= self.isolation_max_retries => {
                    tracing::debug!(
                        target: "cratestack",
                        cratestack_isolation = isolation.as_sql(),
                        cratestack_sqlstate = sqlstate,
                        cratestack_attempt = attempt,
                        "retrying @isolation transaction",
                    );
                    backoff(attempt).await;
                }
                Attempt::Retry(sqlstate) => {
                    return Err(CratestackError::TransactionAborted(
                        TransactionAbort::exhausted(DbErrorInfo {
                            detail: ISOLATION_CONFLICT_DETAIL.to_owned(),
                            sqlstate: Some(sqlstate.to_owned()),
                            constraint: None,
                        }),
                    ));
                }
                Attempt::Failed(error) => return Err(error),
            }
        }
    }
}

async fn finish_attempt<T>(bound: &BoundTx, result: Result<T, CratestackError>) -> Attempt<T> {
    let Some(tx) = bound.take().await else {
        return Attempt::Failed(CratestackError::Internal(
            "@isolation transaction missing at commit".to_owned(),
        ));
    };
    let tx = tx.into_inner();
    let taint = bound.tainted();
    // A body that returned `Ok` over a transaction `db.transaction(..)`
    // could not close cleanly has not succeeded: its `COMMIT` would be a
    // silent `ROLLBACK` (aborted) or a no-op (already ended).
    let result = match (result, bound.take_poison()) {
        (Ok(_), Some(poison)) => Err(poison),
        (result, _) => result,
    };
    match result {
        Ok(value) if taint.is_none() => match tx.commit().await {
            Ok(()) => Attempt::Committed(value),
            Err(error) => {
                let error = cratestack_error_from_sqlx(error);
                match retriable_sqlstate(&error) {
                    Some(sqlstate) => Attempt::Retry(sqlstate),
                    None => Attempt::Failed(error),
                }
            }
        },
        Ok(_) => {
            let _ = tx.rollback().await;
            Attempt::Retry(taint.unwrap_or("40001"))
        }
        Err(error) => {
            let _ = tx.rollback().await;
            match retriable_sqlstate(&error).or(taint) {
                Some(sqlstate) => Attempt::Retry(sqlstate),
                None => Attempt::Failed(error),
            }
        }
    }
}

/// `2ms × 2^(retry-1)`, capped at 64 ms, plus up to as much again of
/// jitter so two contenders that just collided do not retry in lockstep.
async fn backoff(retry: u32) {
    let base_ms = (2u64 << retry.saturating_sub(1).min(5)).min(64);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| u64::from(elapsed.subsec_nanos()))
        .unwrap_or(0);
    let jitter = Duration::from_nanos(nanos % (base_ms * 1_000_000 + 1));
    tokio::time::sleep(Duration::from_millis(base_ms) + jitter).await;
}
