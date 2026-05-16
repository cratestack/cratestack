//! `UpdateMany` / `UpdateManySet` — bulk UPDATE by predicate. Refuses an
//! empty filter list to keep typos from wiping the table.

use std::marker::PhantomData;

use cratestack_core::BatchSummary;
use cratestack_sql::{
    Filter, FilterExpr, ModelDescriptor, SqlValue, SqliteDialect, UpdateModelInput,
};
use rusqlite::params_from_iter;

use crate::{
    FromRusqliteRow, RusqliteError, RusqliteRuntime, SqlValueParam, render::render_update_many,
};

pub struct UpdateMany<'a, M: 'static, PK: 'static> {
    pub(super) runtime: &'a RusqliteRuntime,
    pub(super) descriptor: &'static ModelDescriptor<M, PK>,
    pub(super) filters: Vec<FilterExpr>,
}

impl<'a, M: 'static, PK: 'static> UpdateMany<'a, M, PK> {
    pub fn where_(mut self, filter: Filter) -> Self {
        self.filters.push(FilterExpr::from(filter));
        self
    }

    pub fn where_expr(mut self, filter: FilterExpr) -> Self {
        self.filters.push(filter);
        self
    }

    /// Conditionally append a filter. See
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

    pub fn set<I>(self, input: I) -> UpdateManySet<'a, M, PK, I> {
        UpdateManySet {
            runtime: self.runtime,
            descriptor: self.descriptor,
            filters: self.filters,
            input,
            _marker: PhantomData,
        }
    }
}

pub struct UpdateManySet<'a, M: 'static, PK: 'static, I> {
    pub(super) runtime: &'a RusqliteRuntime,
    pub(super) descriptor: &'static ModelDescriptor<M, PK>,
    pub(super) filters: Vec<FilterExpr>,
    pub(super) input: I,
    pub(super) _marker: PhantomData<fn() -> M>,
}

impl<'a, M: 'static, PK: 'static, I> UpdateManySet<'a, M, PK, I>
where
    I: UpdateModelInput<M>,
{
    pub fn preview_sql(&self) -> String {
        let dialect = SqliteDialect;
        let values = self.input.sql_values();
        let (sql, _) = render_update_many(&dialect, self.descriptor, &values, &self.filters);
        sql
    }

    /// Run the bulk update. Returns a `BatchSummary { total, ok, err: 0 }`
    /// where `ok` is the number of rows the UPDATE actually mutated.
    pub fn run(self) -> Result<BatchSummary, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        let dialect = SqliteDialect;
        if self.filters.is_empty() {
            // Mirror the sqlx safety stance: reject predicate-less bulk
            // updates loud and early. There's no equivalent of
            // `CoolError::Validation` here, so we surface a sqlite error
            // — an empty WHERE would let a typo wipe the table.
            return Err(RusqliteError::Validation(
                "update_many requires at least one filter".to_owned(),
            ));
        }
        let values = self.input.sql_values();
        if values.is_empty() {
            return Err(RusqliteError::Validation(
                "update input must contain at least one changed column".to_owned(),
            ));
        }
        let (sql, binds) = render_update_many(&dialect, self.descriptor, &values, &self.filters);
        self.runtime
            .with_connection(|conn| run_update_many_returning::<M>(conn, &sql, &binds))
    }

    /// Run against a caller-supplied connection. See
    /// [`CreateRecord::run_in_tx`].
    pub fn run_in_tx(self, conn: &rusqlite::Connection) -> Result<BatchSummary, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        if self.filters.is_empty() {
            return Err(RusqliteError::Validation(
                "update_many requires at least one filter".to_owned(),
            ));
        }
        let dialect = SqliteDialect;
        let values = self.input.sql_values();
        if values.is_empty() {
            return Err(RusqliteError::Validation(
                "update input must contain at least one changed column".to_owned(),
            ));
        }
        let (sql, binds) = render_update_many(&dialect, self.descriptor, &values, &self.filters);
        run_update_many_returning::<M>(conn, &sql, &binds)
    }
}

fn run_update_many_returning<M: FromRusqliteRow>(
    conn: &rusqlite::Connection,
    sql: &str,
    binds: &[SqlValue],
) -> Result<BatchSummary, RusqliteError> {
    let mut stmt = conn.prepare(sql)?;
    let bind_iter = binds.iter().map(SqlValueParam);
    let mut count = 0usize;
    {
        // We use query_map but only care about the row count — discarding
        // each row keeps the FromRusqliteRow round-trip honest (catches
        // schema mismatches early) without retaining the materialised set.
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
