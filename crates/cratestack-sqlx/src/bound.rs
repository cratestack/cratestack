//! The transaction a `@isolation` procedure runs in, shared by every
//! operation its handle makes (docs/design/procedure-isolation.md §4).
//!
//! A [`SqlxRuntime`](crate::SqlxRuntime) normally executes on its pool. A
//! runtime built by [`SqlxRuntime::run_isolated`](crate::SqlxRuntime) for one
//! attempt carries an `Arc<BoundTx>` instead, and every executing path
//! checks [`SqlxRuntime::bound`](crate::SqlxRuntime) first: builders run
//! their existing `run_in_tx` body inside a savepoint of this transaction,
//! and post-commit side effects (the `AuditSink` fan-out, the outbox drain)
//! are recorded here and performed only after the attempt commits.

use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicBool, Ordering};

use cratestack_core::{AuditEvent, CratestackError, TransactionIsolation};

use crate::retriable::retriable_sqlstate;
use crate::transaction::Tx;

pub(crate) struct BoundTx {
    tx: tokio::sync::Mutex<Option<Tx>>,
    audit_events: StdMutex<Vec<AuditEvent>>,
    drain_outbox: AtomicBool,
    /// SQLSTATE of the first retriable error any operation observed in
    /// this attempt. Set even when the body later swallows the error: an
    /// attempt that saw one is rolled back and retried, never committed.
    tainted: StdMutex<Option<&'static str>>,
    /// Set when `db.transaction(..)` could not close its savepoint: the
    /// closure left the transaction aborted (a failed statement it did not
    /// propagate) or ended it (raw `COMMIT`/`ROLLBACK`). Postgres answers
    /// `COMMIT` on an aborted transaction with a silent `ROLLBACK`, so such
    /// an attempt is never committed, whatever the body returns (§7).
    poisoned: StdMutex<Option<CratestackError>>,
    /// The level the attempt began at. A nested `@isolation` call joins the
    /// attempt only at this level or a weaker one ([`join_bound`], §7.1).
    isolation: TransactionIsolation,
    /// Whether a nested `@isolation` call is currently joined to this
    /// attempt; at most one is ([`join_bound`], §7.1).
    joined: AtomicBool,
}

pub(crate) struct BoundGuard<'a> {
    guard: tokio::sync::MutexGuard<'a, Option<Tx>>,
}

impl BoundGuard<'_> {
    pub(crate) fn tx(&mut self) -> Result<&mut Tx, CratestackError> {
        self.guard.as_mut().ok_or_else(finished_error)
    }
}

impl BoundTx {
    pub(crate) fn new(tx: Tx, isolation: TransactionIsolation) -> Self {
        Self {
            tx: tokio::sync::Mutex::new(Some(tx)),
            audit_events: StdMutex::new(Vec::new()),
            drain_outbox: AtomicBool::new(false),
            tainted: StdMutex::new(None),
            poisoned: StdMutex::new(None),
            isolation,
            joined: AtomicBool::new(false),
        }
    }

    /// Exclusive use of the transaction for one operation. A Postgres
    /// connection runs one statement at a time, so a second, concurrent
    /// or re-entrant, operation on the same handle is refused instead of
    /// waiting on a lock its own caller holds (§4.2).
    pub(crate) fn lock(&self) -> Result<BoundGuard<'_>, CratestackError> {
        let guard = self.tx.try_lock().map_err(|_| {
            CratestackError::Internal(
                "the @isolation transaction handle is already in use: operations on it must \
                 run one at a time, and inside `db.transaction(|tx| ..)` pass `tx` to \
                 `run_in_tx(tx, ctx)` instead of calling `.run(ctx)`"
                    .to_owned(),
            )
        })?;
        if guard.is_none() {
            return Err(finished_error());
        }
        Ok(BoundGuard { guard })
    }

    /// Record a retriable failure (40001/40P01) seen by any operation.
    pub(crate) fn observe<T>(&self, result: &Result<T, CratestackError>) {
        if let Err(error) = result
            && let Some(code) = retriable_sqlstate(error)
            && let Ok(mut tainted) = self.tainted.lock()
            && tainted.is_none()
        {
            *tainted = Some(code);
        }
    }

    pub(crate) fn tainted(&self) -> Option<&'static str> {
        self.tainted.lock().ok().and_then(|tainted| *tainted)
    }

    pub(crate) fn poison(&self, error: CratestackError) {
        if let Ok(mut poisoned) = self.poisoned.lock()
            && poisoned.is_none()
        {
            *poisoned = Some(error);
        }
    }

    pub(crate) fn take_poison(&self) -> Option<CratestackError> {
        self.poisoned
            .lock()
            .ok()
            .and_then(|mut poisoned| poisoned.take())
    }

    pub(crate) fn defer_audit(&self, events: impl IntoIterator<Item = AuditEvent>) {
        if let Ok(mut deferred) = self.audit_events.lock() {
            deferred.extend(events);
        }
    }

    pub(crate) fn request_drain(&self) {
        self.drain_outbox.store(true, Ordering::SeqCst);
    }

    /// What `run()` on a bound runtime does with a write's `run_in_tx`
    /// outcome: keep its audit events and, when the model emits for this
    /// kind of write, remember to drain the outbox — both after commit.
    pub(crate) fn settle<T>(&self, outcome: crate::RunInTxOutcome<T>, emits: bool) -> T {
        self.defer_audit(outcome.audit_events);
        if emits {
            self.request_drain();
        }
        outcome.value
    }

    pub(crate) fn take_deferred(&self) -> (Vec<AuditEvent>, bool) {
        let events = self
            .audit_events
            .lock()
            .map(|mut deferred| std::mem::take(&mut *deferred))
            .unwrap_or_default();
        (events, self.drain_outbox.load(Ordering::SeqCst))
    }

    /// Take the transaction out for the final commit or rollback. Waits
    /// for the lock rather than refusing: the body has returned, so
    /// nothing legitimate still holds it.
    pub(crate) async fn take(&self) -> Option<Tx> {
        self.tx.lock().await.take()
    }
}

fn finished_error() -> CratestackError {
    CratestackError::Internal(
        "the @isolation transaction has already finished; the handle cannot be used after the \
         procedure returns"
            .to_owned(),
    )
}

mod join;
mod nested;
mod savepoint;

pub(crate) use join::join_bound;
pub(crate) use nested::nested_in_bound;
pub(crate) use savepoint::{begin_savepoint, finish_savepoint, in_bound_savepoint, in_write_tx};
