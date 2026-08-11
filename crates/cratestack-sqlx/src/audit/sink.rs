//! Post-commit fan-out of already-persisted [`AuditEvent`]s to the
//! runtime's installed [`cratestack_core::AuditSink`] (cratestack#473).

use cratestack_core::AuditEvent;

use crate::descriptor::SqlxRuntime;

/// Fan a batch of already-committed [`AuditEvent`]s out to the
/// runtime's installed [`cratestack_core::AuditSink`].
///
/// **Deliberately called after `tx.commit()`, never before or from
/// inside the transaction.** Two reasons:
///
/// 1. **No double-write of the source of truth.** The only DB write is
///    [`super::enqueue_audit_event`], run once, in-transaction, before
///    this is ever reached. This function performs no DB I/O — it only
///    invokes `AuditSink::record`, an out-of-band, best-effort
///    projection. There is exactly one write to `cratestack_audit` per
///    event either way; this cannot cause a second one.
/// 2. **The transaction must not wait on downstream I/O.** A sink can
///    be a Kafka publish, a Redis command, or an HTTP webhook — any of
///    which can be slow or hang. Running that call while still holding
///    the mutation's row locks would turn an unrelated outage (the
///    Kafka broker is down) into a long-held Postgres lock, which is
///    far worse than a late or dropped downstream projection. Waiting
///    for commit also guarantees a sink is only ever invoked for
///    events that actually happened — a rolled-back transaction never
///    reaches this call, so the sink can't observe a mutation the
///    database itself discarded.
///
/// Errors are logged, not propagated: by the time this runs the
/// mutation already committed, so failing the caller's request over a
/// downstream sink hiccup would be strictly worse than a best-effort
/// delivery. This mirrors `run()`'s existing `let _ =
/// self.runtime.drain_event_outbox().await;` treatment of its own
/// post-commit, best-effort fan-out.
///
/// **Not called from any `run_in_tx` variant, and this is a real,
/// currently-unaddressed gap, not a deferred convenience.** `run_in_tx`
/// hands the transaction back to the caller uncommitted, so this
/// function has no reliable "after commit" point to run at — same
/// reason `run_in_tx` never drains the event outbox either (see
/// `crate::query::write::create`'s doc comment). Unlike the event
/// outbox, though, there is currently **no way for a `run_in_tx` caller
/// to opt into sink fan-out themselves**: this function is
/// `pub(crate)`, and no `run_in_tx` variant returns the `AuditEvent` it
/// would need even if it were public. A caller chaining `run_in_tx`
/// calls across a caller-managed transaction (the shape
/// `crates/cratestack-pg/tests/banking_chained_audit_tx.rs` exercises)
/// gets the in-transaction `cratestack_audit` row on commit — the
/// source of truth — but a real installed `AuditSink` observes nothing
/// for that transaction, silently. Closing this needs either widening
/// this function's visibility and every `run_in_tx` signature to return
/// its built `AuditEvent`(s) (a public API change across seven call
/// sites), or a narrower purpose-built opt-in that hasn't been designed
/// yet; tracked as a follow-up rather than fixed inline here.
///
/// **Dispatch is sequential, not concurrent**, and that amplifies with
/// batch size: the `for` loop below `.await`s each `AuditSink::record`
/// call one at a time, so an `update_many`/`delete_many`/`batch_*` call
/// touching N rows makes N sequential post-commit sink calls before the
/// response returns — a slow sink's added latency is per-row, not
/// per-request. Deliberately not parallelised here: concurrent
/// dispatch's ordering guarantees and per-event error semantics are a
/// design question of their own, not a cleanup.
pub(crate) async fn dispatch_audit_sink(runtime: &SqlxRuntime, events: &[AuditEvent]) {
    for event in events {
        if let Err(error) = runtime.audit_sink().record(event).await {
            tracing::warn!(
                error = %error,
                event_id = %event.event_id,
                model = %event.model,
                operation = event.operation.as_str(),
                "audit sink failed to record event",
            );
        }
    }
}
