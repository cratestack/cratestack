//! `AggregateColumn` — SUM/AVG/MIN/MAX over a single column. `T` is the
//! caller-chosen rusqlite-decodable result type.

use cratestack_sql::{Filter, FilterExpr, ModelDescriptor, SqlValue};
use rusqlite::params_from_iter;

use crate::{RusqliteError, RusqliteRuntime, SqlValueParam};

use super::aggregate::{AggregateOp, AggregateProjection, render_aggregate};

pub struct AggregateColumn<'a, M: 'static, PK: 'static> {
    runtime: &'a RusqliteRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    op: AggregateOp,
    column: &'static str,
    filters: Vec<FilterExpr>,
}

impl<'a, M: 'static, PK: 'static> AggregateColumn<'a, M, PK> {
    pub(super) fn new<C: cratestack_sql::IntoColumnName>(
        runtime: &'a RusqliteRuntime,
        descriptor: &'static ModelDescriptor<M, PK>,
        op: AggregateOp,
        column: C,
    ) -> Self {
        Self {
            runtime,
            descriptor,
            op,
            column: column.into_column_name(),
            filters: Vec::new(),
        }
    }

    pub fn where_(mut self, filter: Filter) -> Self {
        self.filters.push(FilterExpr::from(filter));
        self
    }

    pub fn where_expr(mut self, filter: FilterExpr) -> Self {
        self.filters.push(filter);
        self
    }

    pub fn where_optional<F>(mut self, filter: Option<F>) -> Self
    where
        F: Into<FilterExpr>,
    {
        if let Some(filter) = filter {
            self.filters.push(filter.into());
        }
        self
    }

    fn render(&self) -> (String, Vec<SqlValue>) {
        render_aggregate(
            self.descriptor,
            AggregateProjection::Column {
                function: self.op.function_name(),
                column: self.column,
            },
            &self.filters,
        )
    }

    /// Run the aggregate. `T` is whatever `rusqlite::types::FromSql`-shaped
    /// scalar the call site wants — `i64` for `SUM(int)`, `f64` for
    /// `AVG(int)`, `chrono::DateTime` for `MIN(timestamp)`, etc.
    pub fn run<T>(self) -> Result<Option<T>, RusqliteError>
    where
        T: rusqlite::types::FromSql,
    {
        let (sql, binds) = self.render();
        self.runtime.with_connection(|conn| {
            let mut stmt = conn.prepare(&sql)?;
            let bind_iter = binds.iter().map(SqlValueParam);
            let value: Option<T> =
                stmt.query_row(params_from_iter(bind_iter), |row| row.get(0))?;
            Ok(value)
        })
    }

    pub fn run_in_tx<T>(self, conn: &rusqlite::Connection) -> Result<Option<T>, RusqliteError>
    where
        T: rusqlite::types::FromSql,
    {
        let (sql, binds) = self.render();
        let mut stmt = conn.prepare(&sql)?;
        let bind_iter = binds.iter().map(SqlValueParam);
        let value: Option<T> =
            stmt.query_row(params_from_iter(bind_iter), |row| row.get(0))?;
        Ok(value)
    }
}
