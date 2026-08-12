//! [`RunInTxOutcome`] — what every write builder's `run_in_tx` returns
//! (cratestack#534). It pairs the normal `.run(..)`-equivalent return
//! value with the [`AuditEvent`]s the call already built and persisted
//! to `cratestack_audit` inside the caller's transaction, so a caller
//! who owns that transaction can fan them out to the installed
//! `AuditSink` once *they* commit. See [`super::sink::dispatch_audit_sink`]'s
//! doc comment for the full contract this exists to serve, and why
//! `run_in_tx` cannot dispatch the events itself.

use cratestack_core::AuditEvent;

/// See the module doc comment.
#[derive(Debug, Clone)]
pub struct RunInTxOutcome<T> {
    /// Exactly what the equivalent `.run(..)` call would have
    /// returned.
    pub value: T,
    /// `AuditEvent`s already persisted to `cratestack_audit` by this
    /// call, in mutation order. Empty when the model isn't
    /// `@@audit`-enabled, or (for `.upsert(..).do_nothing()`) when the
    /// conflict target already existed and nothing was written.
    /// Single-row writes build at most one; `update_many` /
    /// `delete_many` build one per matched row.
    pub audit_events: Vec<AuditEvent>,
}

impl<T> RunInTxOutcome<T> {
    pub(crate) fn new(value: T, audit_events: Vec<AuditEvent>) -> Self {
        Self {
            value,
            audit_events,
        }
    }
}
