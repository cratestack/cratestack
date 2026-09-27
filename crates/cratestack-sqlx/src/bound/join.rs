//! A nested `@isolation` call joins the attempt it runs in
//! (docs/design/procedure-isolation.md §7.1).
//!
//! `SqlxRuntime::run_isolated` on a runtime already bound to an attempt —
//! a `@computed` resolver of an `@isolation` procedure's output calling
//! another `@isolation` procedure's `invoke_with_db` with the `&Cratestack`
//! it was given — used to begin a second pool transaction. That inner
//! transaction committed on its own, before the outer one did: an outer
//! rollback or retry kept its writes, and each retry committed them again.
//! It now runs as a savepoint of the outer attempt, at the outer attempt's
//! level, and only the outermost attempt retries.

use std::future::Future;
use std::sync::atomic::Ordering;

use cratestack_core::{CratestackError, TransactionIsolation};

use super::BoundTx;
use crate::descriptor::SqlxRuntime;
use crate::error::cratestack_error_from_sqlx;
use crate::sqlx;

/// The joined call's savepoint. One name is enough: at most one joined call
/// runs on an attempt at a time.
const JOIN_SAVEPOINT: &str = "cratestack_isolated_join";

/// Run `body` once, in a savepoint of `bound`'s attempt. Refused, before
/// anything runs, when `isolation` is stricter than the attempt's level:
/// Postgres cannot raise a transaction's level once it has begun, and
/// running the inner procedure at a weaker level than it declared would be
/// the defect GHSA-r67q-4qqq-g9gm fixed. A weaker or equal declared level
/// runs at the attempt's (stricter) level.
///
/// Never retries and never produces `TransactionAborted`: a retriable
/// failure taints the outer attempt, which rolls back and re-runs the whole
/// thing — this call included. On `Err` the savepoint is rolled back and
/// the audit events queued inside it are dropped with its writes.
pub(crate) async fn join_bound<F, Fut, T>(
    runtime: &SqlxRuntime,
    bound: &BoundTx,
    isolation: TransactionIsolation,
    body: F,
) -> Result<T, CratestackError>
where
    F: FnOnce(SqlxRuntime) -> Fut,
    Fut: Future<Output = Result<T, CratestackError>>,
{
    if strength(isolation) > strength(bound.isolation) {
        return Err(CratestackError::Internal(format!(
            "an @isolation(\"{inner}\") procedure was called inside an @isolation(\"{outer}\") \
             transaction; a transaction's isolation level cannot be raised after it has begun. \
             Declare the calling procedure {inner} or stricter, or call this one outside it",
            inner = isolation.as_sql(),
            outer = bound.isolation.as_sql(),
        )));
    }
    // One joined call at a time, refused at the start rather than detected
    // at the end. Two running concurrently on the attempt (`tokio::join!` in
    // a resolver) interleave their operations between the savepoints: the
    // later one's `ROLLBACK TO` undoes the earlier one's writes made after
    // it began, while that one still reports `Ok`, and `truncate_audit`
    // drops its audit events. A check at the end catches only one of the
    // two completion orders. A call started from inside a joined call (the
    // resolver's handle captured into its closure) is indistinguishable from
    // a concurrent one — both use the attempt's root handle — and is refused
    // the same way.
    if bound
        .joined
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        let error = CratestackError::Internal(
            "a nested @isolation call started while another one was still running on the same \
             transaction (concurrently, or from inside it); joined calls must run one at a time"
                .to_owned(),
        );
        poison(bound, &error);
        return Err(error);
    }
    let mut guard = JoinGuard {
        bound,
        finished: false,
    };
    let mark = bound.audit_mark();
    let begun = statement(bound, "SAVEPOINT", JOIN_SAVEPOINT).await;
    bound.observe(&begun);
    if let Err(error) = begun {
        guard.finished = true;
        return Err(error);
    }
    let result = body(runtime.clone())
        .await
        .map_err(CratestackError::propagate_transaction_abort);
    let result = match result {
        Ok(value) => match statement(bound, "RELEASE SAVEPOINT", JOIN_SAVEPOINT).await {
            Ok(()) => Ok(value),
            Err(error) => {
                poison(bound, &error);
                Err(error)
            }
        },
        Err(error) => {
            bound.truncate_audit(mark);
            let rolled_back = match statement(bound, "ROLLBACK TO SAVEPOINT", JOIN_SAVEPOINT).await
            {
                Ok(()) => statement(bound, "RELEASE SAVEPOINT", JOIN_SAVEPOINT).await,
                Err(rollback_error) => Err(rollback_error),
            };
            if let Err(rollback_error) = rolled_back {
                poison(bound, &rollback_error);
            }
            Err(error)
        }
    };
    guard.finished = true;
    bound.observe(&result);
    result
}

/// Clears [`BoundTx::joined`] however the joined call ends. Dropped before
/// it finished (the caller cancelled it with a timeout or `select!`, or it
/// panicked), the savepoint is still open with whatever the call wrote so
/// far, and committing the attempt would keep that half: it is poisoned.
struct JoinGuard<'a> {
    bound: &'a BoundTx,
    finished: bool,
}

impl Drop for JoinGuard<'_> {
    fn drop(&mut self) {
        if !self.finished {
            poison(
                self.bound,
                &CratestackError::Internal(
                    "a nested @isolation call was dropped before it finished".to_owned(),
                ),
            );
        }
        self.bound.joined.store(false, Ordering::SeqCst);
    }
}

/// Order of strictness. Postgres runs `READ UNCOMMITTED` as `READ
/// COMMITTED`, and `TransactionIsolation` has no such variant.
fn strength(level: TransactionIsolation) -> u8 {
    match level {
        TransactionIsolation::ReadCommitted => 0,
        TransactionIsolation::RepeatableRead => 1,
        TransactionIsolation::Serializable => 2,
    }
}

async fn statement(bound: &BoundTx, verb: &str, name: &str) -> Result<(), CratestackError> {
    let mut guard = bound.lock()?;
    let tx = guard.tx()?;
    // `AssertSqlSafe`: `verb` is one of three literals and `name` is
    // `JOIN_SAVEPOINT`; nothing request-derived.
    sqlx::query(sqlx::AssertSqlSafe(format!("{verb} {name}")))
        .execute(&mut ***tx)
        .await
        .map(|_| ())
        .map_err(cratestack_error_from_sqlx)
}

fn poison(bound: &BoundTx, cause: &CratestackError) {
    tracing::warn!(
        target: "cratestack",
        cratestack_error = cause.code(),
        "a nested @isolation call could not close its savepoint; the attempt will not be \
         committed",
    );
    bound.poison(CratestackError::Internal(format!(
        "a nested @isolation call could not close its savepoint ({}); the procedure's work was \
         rolled back",
        cause.code(),
    )));
}

impl BoundTx {
    /// How many audit events are queued, to drop the ones a rolled-back
    /// joined call queued ([`Self::truncate_audit`]).
    fn audit_mark(&self) -> usize {
        self.audit_events
            .lock()
            .map(|events| events.len())
            .unwrap_or(0)
    }

    fn truncate_audit(&self, mark: usize) {
        if let Ok(mut events) = self.audit_events.lock() {
            events.truncate(mark);
        }
    }
}
