//! Dialect-specific SQL emission for the migration IR.
//!
//! Each emitter consumes a `&[Op]` and produces an [`EmittedMigration`]:
//! the `up.sql` body, and a `down.sql` body that either reverses
//! every op or contains an explicit error stub when reversal would
//! lose data.
//!
//! The IR itself stays dialect-agnostic — emitters own all
//! type-mapping, identifier-quoting, and per-dialect quirks.

pub mod postgres;
pub mod sqlite;

/// Output of an emitter run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmittedMigration {
    /// SQL applied to roll the migration forward.
    pub up: String,
    /// SQL applied to roll the migration back. For migrations
    /// containing lossy ops, this is an explicit error stub rather
    /// than reverse SQL — the runner refuses to execute it and the
    /// developer must hand-write any reversal that destroys data.
    pub down: String,
    /// Whether the migration contains any lossy ops. Useful for the
    /// CLI to gate on `--allow-destructive`.
    pub has_lossy: bool,
    /// Whether the migration contains any blocking ops. The CLI uses
    /// this to surface a clear "needs `@default` or `up.pre.sql`"
    /// warning.
    pub has_blocking: bool,
}
