//! `UpdateRecord` / `UpdateRecordSet` — `.update(id).set(patch).run()`
//! single-row update builder.

use std::marker::PhantomData;

use cratestack_sql::{IntoSqlValue, ModelDescriptor, SqliteDialect, UpdateModelInput};

use crate::{FromRusqliteRow, RusqliteError, RusqliteRuntime, render::render_update};

use super::support::run_insert_returning;

pub struct UpdateRecord<'a, M: 'static, PK: 'static> {
    pub(super) runtime: &'a RusqliteRuntime,
    pub(super) descriptor: &'static ModelDescriptor<M, PK>,
    pub(super) id: PK,
}

impl<'a, M: 'static, PK: 'static> UpdateRecord<'a, M, PK> {
    pub fn set<I>(self, input: I) -> UpdateRecordSet<'a, M, PK, I> {
        UpdateRecordSet {
            runtime: self.runtime,
            descriptor: self.descriptor,
            id: self.id,
            input,
            _marker: PhantomData,
        }
    }
}

pub struct UpdateRecordSet<'a, M: 'static, PK: 'static, I> {
    pub(super) runtime: &'a RusqliteRuntime,
    pub(super) descriptor: &'static ModelDescriptor<M, PK>,
    pub(super) id: PK,
    pub(super) input: I,
    pub(super) _marker: PhantomData<fn() -> M>,
}

impl<'a, M: 'static, PK: 'static, I> UpdateRecordSet<'a, M, PK, I>
where
    PK: IntoSqlValue + Clone,
    I: UpdateModelInput<M>,
{
    pub fn preview_sql(&self) -> String {
        let dialect = SqliteDialect;
        let values = self.input.sql_values();
        let (sql, _) = render_update(
            &dialect,
            self.descriptor,
            &values,
            self.id.clone().into_sql_value(),
        );
        sql
    }

    pub fn run(self) -> Result<M, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        let dialect = SqliteDialect;
        let values = self.input.sql_values();
        let (sql, binds) = render_update(
            &dialect,
            self.descriptor,
            &values,
            self.id.clone().into_sql_value(),
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
        let (sql, binds) = render_update(
            &dialect,
            self.descriptor,
            &values,
            self.id.clone().into_sql_value(),
        );
        run_insert_returning(conn, &sql, &binds)
    }
}
