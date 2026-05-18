//! `FindUnique` — single-row PK lookup with optional `.select(...)`
//! projection.

use cratestack_sql::{IntoSqlValue, ReadSource, SqliteDialect};
use rusqlite::params_from_iter;

use crate::{
    FromRusqliteRow, RusqliteError, RusqliteRuntime, SqlValueParam, render::render_select_by_pk,
};

use super::projected_find_unique::ProjectedFindUnique;

pub struct FindUnique<'a, M: 'static, PK: 'static> {
    pub(super) runtime: &'a RusqliteRuntime,
    pub(super) descriptor: &'static dyn ReadSource<M, PK>,
    pub(super) id: PK,
}

impl<'a, M: 'static, PK: 'static> FindUnique<'a, M, PK>
where
    PK: IntoSqlValue + Clone,
{
    /// See [`FindMany::for_update`] — no-op on the embedded layer.
    pub fn for_update(self) -> Self {
        self
    }

    /// API-compat no-op. The embedded layer doesn't enforce policies,
    /// so the detail-vs-list distinction has no runtime effect; kept
    /// so cross-backend code can call `.as_detail()` / `.as_list()`
    /// without conditional compilation.
    pub fn as_detail(self) -> Self {
        self
    }

    /// API-compat no-op. See [`Self::as_detail`].
    pub fn as_list(self) -> Self {
        self
    }

    pub fn preview_sql(&self) -> String {
        let dialect = SqliteDialect;
        let (sql, _) =
            render_select_by_pk(&dialect, self.descriptor, self.id.clone().into_sql_value());
        sql
    }

    pub fn run(self) -> Result<Option<M>, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        let dialect = SqliteDialect;
        let (sql, binds) =
            render_select_by_pk(&dialect, self.descriptor, self.id.clone().into_sql_value());
        self.runtime.with_connection(|conn| {
            let mut stmt = conn.prepare(&sql)?;
            let bind_iter = binds.iter().map(SqlValueParam);
            let mut rows = stmt.query(params_from_iter(bind_iter))?;
            if let Some(row) = rows.next()? {
                Ok(Some(M::from_rusqlite_row(row)?))
            } else {
                Ok(None)
            }
        })
    }

    /// Run against a caller-supplied connection. See
    /// [`FindMany::run_in_tx`] for cross-backend rationale.
    pub fn run_in_tx(self, conn: &rusqlite::Connection) -> Result<Option<M>, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        let dialect = SqliteDialect;
        let (sql, binds) =
            render_select_by_pk(&dialect, self.descriptor, self.id.clone().into_sql_value());
        let mut stmt = conn.prepare(&sql)?;
        let bind_iter = binds.iter().map(SqlValueParam);
        let mut rows = stmt.query(params_from_iter(bind_iter))?;
        if let Some(row) = rows.next()? {
            Ok(Some(M::from_rusqlite_row(row)?))
        } else {
            Ok(None)
        }
    }

    /// Restrict the SELECT to the named columns; see
    /// [`cratestack_sqlx::FindUnique::select`] for the shared
    /// caller-side contract. Returns `Option<Projection<M>>`.
    pub fn select<I, C>(self, columns: I) -> ProjectedFindUnique<'a, M, PK>
    where
        I: IntoIterator<Item = C>,
        C: cratestack_sql::IntoColumnName,
    {
        ProjectedFindUnique {
            runtime: self.runtime,
            descriptor: self.descriptor,
            id: self.id,
            selected: columns
                .into_iter()
                .map(cratestack_sql::IntoColumnName::into_column_name)
                .collect(),
        }
    }
}
