//! `DeleteRecord` — single-row DELETE / soft-delete UPDATE RETURNING.

use cratestack_sql::{IntoSqlValue, ModelDescriptor, SqliteDialect};

use crate::{
    FromRusqliteRow, RusqliteError, RusqliteRuntime, render::render_delete,
};

use super::support::run_insert_returning;

pub struct DeleteRecord<'a, M: 'static, PK: 'static> {
    pub(super) runtime: &'a RusqliteRuntime,
    pub(super) descriptor: &'static ModelDescriptor<M, PK>,
    pub(super) id: PK,
}

impl<'a, M: 'static, PK: 'static> DeleteRecord<'a, M, PK>
where
    PK: IntoSqlValue + Clone,
{
    pub fn preview_sql(&self) -> String {
        let dialect = SqliteDialect;
        let (sql, _) = render_delete(
            &dialect,
            self.descriptor,
            self.id.clone().into_sql_value(),
            chrono::Utc::now(),
        );
        sql
    }

    pub fn run(self) -> Result<M, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        let dialect = SqliteDialect;
        let (sql, binds) = render_delete(
            &dialect,
            self.descriptor,
            self.id.clone().into_sql_value(),
            chrono::Utc::now(),
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
        let (sql, binds) = render_delete(
            &dialect,
            self.descriptor,
            self.id.clone().into_sql_value(),
            chrono::Utc::now(),
        );
        run_insert_returning(conn, &sql, &binds)
    }
}
