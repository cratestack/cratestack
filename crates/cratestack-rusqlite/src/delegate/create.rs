//! `CreateRecord` — single-row INSERT RETURNING with pool and txn variants.

use cratestack_sql::{CreateModelInput, ModelDescriptor, SqliteDialect};

use crate::{
    FromRusqliteRow, RusqliteError, RusqliteRuntime, render::render_insert,
};

use super::support::run_insert_returning;

pub struct CreateRecord<'a, M: 'static, PK: 'static, I> {
    pub(super) runtime: &'a RusqliteRuntime,
    pub(super) descriptor: &'static ModelDescriptor<M, PK>,
    pub(super) input: I,
}

impl<'a, M: 'static, PK: 'static, I> CreateRecord<'a, M, PK, I>
where
    I: CreateModelInput<M>,
{
    pub fn preview_sql(&self) -> String {
        let dialect = SqliteDialect;
        let values = self.input.sql_values();
        let (sql, _) = render_insert(&dialect, self.descriptor, &values);
        sql
    }

    pub fn run(self) -> Result<M, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        let dialect = SqliteDialect;
        let values = self.input.sql_values();
        let (sql, binds) = render_insert(&dialect, self.descriptor, &values);
        self.runtime.with_connection(|conn| {
            run_insert_returning(conn, &sql, &binds)
        })
    }

    /// Run against a caller-supplied connection (typically the active
    /// transaction's connection, via `&mut *tx`). Mirrors the sqlx
    /// `run_in_tx` shape so cross-backend code can switch backends
    /// without rewriting transaction call sites.
    pub fn run_in_tx(self, conn: &rusqlite::Connection) -> Result<M, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        let dialect = SqliteDialect;
        let values = self.input.sql_values();
        let (sql, binds) = render_insert(&dialect, self.descriptor, &values);
        run_insert_returning(conn, &sql, &binds)
    }
}
