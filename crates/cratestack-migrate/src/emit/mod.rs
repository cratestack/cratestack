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
    /// Scaffolded `up.pre.sql` body — preparatory SQL the operator
    /// fills in, run before [`Self::up`] in the same transaction by
    /// `cratestack_sqlx::apply_pending`.
    ///
    /// `Some` only when the migration contains a blocking op *and* the
    /// backend has a runner that would execute the file. That second
    /// condition is the whole point: emitting a file nothing reads is
    /// the bug this field was added to fix (cratestack#843), so the
    /// SQLite emitter — which has no migration runner in cratestack at
    /// all — leaves this `None` and puts its guidance in `up.sql`
    /// instead.
    pub up_pre: Option<String>,
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
    /// this to label the migration it wrote. Equivalent to
    /// `!crate::ir::blocking_reasons(ops).is_empty()` — that function
    /// is what to reach for when the *reason* matters and not just the
    /// bit.
    pub has_blocking: bool,
    /// `(table, column)` pairs for `Required` columns using
    /// `@default(dbgenerated())` that this migration introduces. See
    /// [`crate::ir::unverified_dbgenerated_columns`] — non-empty
    /// means the CLI should warn that these columns need a real
    /// Postgres-level default set some other way, or inserts that
    /// omit them will fail with a `NOT NULL` violation at runtime.
    pub unverified_dbgenerated: Vec<(String, String)>,
}
