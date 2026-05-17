//! `DeleteMany` — bulk DELETE / soft-delete by predicate. Empty-filter
//! safety check matches `UpdateMany`.

use cratestack_core::BatchSummary;
use cratestack_sql::{Filter, FilterExpr, ModelDescriptor, SqlValue, SqliteDialect};
use rusqlite::params_from_iter;

use crate::{
    FromRusqliteRow, RusqliteError, RusqliteRuntime, SqlValueParam, render::render_delete_many,
};

pub struct DeleteMany<'a, M: 'static, PK: 'static> {
    pub(super) runtime: &'a RusqliteRuntime,
    pub(super) descriptor: &'static ModelDescriptor<M, PK>,
    pub(super) filters: Vec<FilterExpr>,
}

impl<'a, M: 'static, PK: 'static> DeleteMany<'a, M, PK> {
    pub fn where_(mut self, filter: Filter) -> Self {
        self.filters.push(FilterExpr::from(filter));
        self
    }

    pub fn where_expr(mut self, filter: FilterExpr) -> Self {
        self.filters.push(filter);
        self
    }

    /// Conditionally append a filter; `None` is a no-op. See
    /// [`FindMany::where_optional`].
    pub fn where_optional<F>(mut self, filter: Option<F>) -> Self
    where
        F: Into<FilterExpr>,
    {
        if let Some(filter) = filter {
            self.filters.push(filter.into());
        }
        self
    }

    pub fn preview_sql(&self) -> String {
        let dialect = SqliteDialect;
        let (sql, _) = render_delete_many(&dialect, self.descriptor, &self.filters);
        sql
    }

    pub fn run(self) -> Result<BatchSummary, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        if self.filters.is_empty() {
            return Err(RusqliteError::Validation(
                "delete_many requires at least one filter".to_owned(),
            ));
        }
        let dialect = SqliteDialect;
        let (sql, binds) = render_delete_many(&dialect, self.descriptor, &self.filters);
        self.runtime
            .with_connection(|conn| run_delete_many_returning::<M>(conn, &sql, &binds))
    }

    /// Run against a caller-supplied connection. See
    /// [`CreateRecord::run_in_tx`].
    pub fn run_in_tx(self, conn: &rusqlite::Connection) -> Result<BatchSummary, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        if self.filters.is_empty() {
            return Err(RusqliteError::Validation(
                "delete_many requires at least one filter".to_owned(),
            ));
        }
        let dialect = SqliteDialect;
        let (sql, binds) = render_delete_many(&dialect, self.descriptor, &self.filters);
        run_delete_many_returning::<M>(conn, &sql, &binds)
    }
}

fn run_delete_many_returning<M: FromRusqliteRow>(
    conn: &rusqlite::Connection,
    sql: &str,
    binds: &[SqlValue],
) -> Result<BatchSummary, RusqliteError> {
    let mut stmt = conn.prepare(sql)?;
    let bind_iter = binds.iter().map(SqlValueParam);
    let mut count = 0usize;
    {
        let iter = stmt.query_map(params_from_iter(bind_iter), |row| {
            M::from_rusqlite_row(row).map(|_| ())
        })?;
        for item in iter {
            item?;
            count += 1;
        }
    }
    Ok(BatchSummary {
        total: count,
        ok: count,
        err: 0,
    })
}
