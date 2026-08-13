//! Post-commit fan-out of already-persisted [`AuditEvent`]s to the
//! runtime's installed [`cratestack_core::AuditSink`] (cratestack#473).

use cratestack_core::AuditEvent;

use crate::descriptor::SqlxRuntime;

/// Fan a batch of already-committed [`AuditEvent`]s out to the
/// runtime's installed [`cratestack_core::AuditSink`].
///
/// `pub`, not `pub(crate)` (cratestack#534): every `run()` call site in
/// this crate calls it internally, right after its own `tx.commit()`
/// succeeds — that usage is unaffected. What's new is that a caller
/// composing `run_in_tx` writes across a transaction *they* own can
/// now call this too, once *their* commit succeeds, passing the
/// `AuditEvent`s each call's [`super::RunInTxOutcome`] handed back.
/// Nothing about the dispatch itself changed: it is still a plain
/// sequential fan-out with no DB I/O of its own (see below) — only who
/// is allowed to invoke it changed.
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
/// **Still not called from any `run_in_tx` variant — that remains a
/// deliberate omission, not an oversight (cratestack#534).** `run_in_tx`
/// hands the transaction back to the caller uncommitted, so this crate
/// has no reliable "after commit" point of its own to run at — same
/// reason `run_in_tx` never drains the event outbox on its own either
/// (see `crate::query::write::create`'s doc comment, and
/// [`crate::SqlxRuntime::drain_event_outbox`] for that mechanism's own
/// equivalent, now-public opt-in). What changed is that a `run_in_tx`
/// caller now genuinely *can* opt in: every `run_in_tx` variant returns
/// a [`super::RunInTxOutcome`] carrying the `AuditEvent`(s) it built and
/// already persisted, and this function is `pub` so the caller can pass
/// them straight through — after their own `tx.commit()` succeeds, never
/// before. The generated `Cratestack::dispatch_audit_sink` method is the
/// ergonomic surface for that; this free function is what it forwards
/// to. Skipping the call (or forgetting it) means exactly what option
/// (c) in cratestack#534 describes: `cratestack_audit` still gets the
/// row, but the installed `AuditSink` observes nothing for that
/// transaction — silently, same as before this fix, just opt-out now
/// instead of impossible-to-opt-into.
/// `crates/cratestack-pg/tests/banking_chained_audit_tx.rs` is the shape
/// this closes: two `run_in_tx` writes chained in one caller-managed
/// transaction, both audited, both now observable by a sink the caller
/// dispatches to once after their single `tx.commit()`.
///
/// **Dispatch is sequential, not concurrent**, and that amplifies with
/// batch size: the `for` loop below `.await`s each `AuditSink::record`
/// call one at a time, so an `update_many`/`delete_many`/`batch_*` call
/// touching N rows makes N sequential post-commit sink calls before the
/// response returns — a slow sink's added latency is per-row, not
/// per-request. Deliberately not parallelised here: concurrent
/// dispatch's ordering guarantees and per-event error semantics are a
/// design question of their own, not a cleanup.
pub async fn dispatch_audit_sink(runtime: &SqlxRuntime, events: &[AuditEvent]) {
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
