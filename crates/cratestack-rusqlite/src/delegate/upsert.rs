//! `UpsertRecord` — INSERT ON CONFLICT DO UPDATE with configurable target.

use cratestack_sql::{ConflictTarget, ModelDescriptor, SqliteDialect, UpsertModelInput};

use crate::{FromRusqliteRow, RusqliteError, RusqliteRuntime, render::render_upsert_with_conflict};

use super::support::run_insert_returning;

/// Reject a predicate paired with `ConflictTarget::PRIMARY_KEY`
/// (cratestack#741) before any SQL is built. `ConflictTarget::validate`
/// returns a `cratestack_core::CratestackError`, shared with the sqlx
/// backend's identical check in `prepare_upsert_insert`; this just
/// re-wraps the message as a `RusqliteError::Validation` so the
/// embedded runtime's error type stays self-contained.
fn conflict_target_validate(target: ConflictTarget) -> Result<(), RusqliteError> {
    target
        .validate()
        .map_err(|error| RusqliteError::Validation(error.to_string()))
}

pub struct UpsertRecord<'a, M: 'static, PK: 'static, I> {
    pub(super) runtime: &'a RusqliteRuntime,
    pub(super) descriptor: &'static ModelDescriptor<M, PK>,
    pub(super) input: I,
    pub(super) conflict_target: ConflictTarget,
}

impl<'a, M: 'static, PK: 'static, I> UpsertRecord<'a, M, PK, I>
where
    I: UpsertModelInput<M>,
{
    /// Choose the conflict target. See
    /// [`cratestack_sqlx::UpsertRecord::on_conflict`]; the embedded
    /// runtime supports `ConflictTarget::columns(...)` symmetrically.
    pub fn on_conflict(mut self, target: ConflictTarget) -> Self {
        self.conflict_target = target;
        self
    }

    pub fn preview_sql(&self) -> String {
        let dialect = SqliteDialect;
        let values = self.input.sql_values();
        let (sql, _) =
            render_upsert_with_conflict(&dialect, self.descriptor, &values, self.conflict_target);
        sql
    }

    pub fn run(self) -> Result<M, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        // Validation is server-side concern only; the rusqlite layer matches
        // `CreateRecord::run`, which also skips `validate()`. The
        // `ConflictTarget` predicate/PK-target combination check
        // (cratestack#741) is NOT a server-only concern, though — it
        // catches a caller error before any SQL runs on either
        // backend — so it still runs here.
        conflict_target_validate(self.conflict_target)?;
        let dialect = SqliteDialect;
        let values = self.input.sql_values();
        let (sql, binds) =
            render_upsert_with_conflict(&dialect, self.descriptor, &values, self.conflict_target);
        self.runtime
            .with_connection(|conn| run_insert_returning(conn, &sql, &binds))
    }

    /// Run against a caller-supplied connection. See
    /// [`CreateRecord::run_in_tx`].
    pub fn run_in_tx(self, conn: &rusqlite::Connection) -> Result<M, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        conflict_target_validate(self.conflict_target)?;
        let dialect = SqliteDialect;
        let values = self.input.sql_values();
        let (sql, binds) =
            render_upsert_with_conflict(&dialect, self.descriptor, &values, self.conflict_target);
        run_insert_returning(conn, &sql, &binds)
    }
}
