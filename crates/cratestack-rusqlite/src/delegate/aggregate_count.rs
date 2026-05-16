//! `AggregateCount` — `COUNT(*)` over the filtered, non-soft-deleted set.

use cratestack_sql::{Filter, FilterExpr, ModelDescriptor, SqlValue};
use rusqlite::params_from_iter;

use crate::{RusqliteError, RusqliteRuntime, SqlValueParam};

use super::aggregate::{AggregateProjection, render_aggregate};

pub struct AggregateCount<'a, M: 'static, PK: 'static> {
    pub(super) runtime: &'a RusqliteRuntime,
    pub(super) descriptor: &'static ModelDescriptor<M, PK>,
    pub(super) filters: Vec<FilterExpr>,
}

impl<'a, M: 'static, PK: 'static> AggregateCount<'a, M, PK> {
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
            AggregateProjection::CountStar,
            &self.filters,
        )
    }

    pub fn run(self) -> Result<i64, RusqliteError> {
        let (sql, binds) = self.render();
        self.runtime.with_connection(|conn| {
            let mut stmt = conn.prepare(&sql)?;
            let bind_iter = binds.iter().map(SqlValueParam);
            let value: i64 = stmt.query_row(params_from_iter(bind_iter), |row| row.get(0))?;
            Ok(value)
        })
    }

    pub fn run_in_tx(self, conn: &rusqlite::Connection) -> Result<i64, RusqliteError> {
        let (sql, binds) = self.render();
        let mut stmt = conn.prepare(&sql)?;
        let bind_iter = binds.iter().map(SqlValueParam);
        let value: i64 = stmt.query_row(params_from_iter(bind_iter), |row| row.get(0))?;
        Ok(value)
    }
}
