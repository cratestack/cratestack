//! `UpsertRecord` — INSERT ON CONFLICT DO UPDATE with configurable target.

use cratestack_sql::{ConflictTarget, ModelDescriptor, SqliteDialect, UpsertModelInput};

use crate::{
    FromRusqliteRow, RusqliteError, RusqliteRuntime, render::render_upsert_with_conflict,
};

use super::support::run_insert_returning;

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
    /// runtime supports `ConflictTarget::Columns` symmetrically.
    pub fn on_conflict(mut self, target: ConflictTarget) -> Self {
        self.conflict_target = target;
        self
    }

    pub fn preview_sql(&self) -> String {
        let dialect = SqliteDialect;
        let values = self.input.sql_values();
        let (sql, _) = render_upsert_with_conflict(
            &dialect,
            self.descriptor,
            &values,
            self.conflict_target,
        );
        sql
    }

    pub fn run(self) -> Result<M, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        // Validation is server-side concern only; the rusqlite layer matches
        // `CreateRecord::run`, which also skips `validate()`.
        let dialect = SqliteDialect;
        let values = self.input.sql_values();
        let (sql, binds) = render_upsert_with_conflict(
            &dialect,
            self.descriptor,
            &values,
            self.conflict_target,
        );
        self.runtime
            .with_connection(|conn| run_insert_returning(conn, &sql, &binds))
    }

    /// Run against a caller-supplied connection. See
    /// [`CreateRecord::run_in_tx`].
    pub fn run_in_tx(self, conn: &rusqlite::Connection) -> Result<M, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        let dialect = SqliteDialect;
        let values = self.input.sql_values();
        let (sql, binds) = render_upsert_with_conflict(
            &dialect,
            self.descriptor,
            &values,
            self.conflict_target,
        );
        run_insert_returning(conn, &sql, &binds)
    }
}
