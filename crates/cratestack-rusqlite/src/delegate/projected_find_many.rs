//! `ProjectedFindMany` — `.select(...)` on a `FindMany`, returns a vec of
//! partial-row `Projection<M>` values.

use cratestack_sql::{Filter, FilterExpr, ModelDescriptor, OrderClause, SqliteDialect};
use rusqlite::params_from_iter;

use crate::{RusqliteError, RusqliteRuntime, SqlValueParam};

use super::projected_select::build_partial_select;

pub struct ProjectedFindMany<'a, M: 'static, PK: 'static> {
    pub(super) runtime: &'a RusqliteRuntime,
    pub(super) descriptor: &'static ModelDescriptor<M, PK>,
    pub(super) filters: Vec<FilterExpr>,
    pub(super) order_by: Vec<OrderClause>,
    pub(super) limit: Option<i64>,
    pub(super) offset: Option<i64>,
    pub(super) selected: Vec<&'static str>,
}

impl<'a, M: 'static, PK: 'static> ProjectedFindMany<'a, M, PK> {
    pub fn where_(mut self, filter: Filter) -> Self {
        self.filters.push(FilterExpr::from(filter));
        self
    }

    pub fn where_expr(mut self, filter: FilterExpr) -> Self {
        self.filters.push(filter);
        self
    }

    pub fn order_by(mut self, clause: OrderClause) -> Self {
        self.order_by.push(clause);
        self
    }

    pub fn limit(mut self, limit: i64) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn offset(mut self, offset: i64) -> Self {
        self.offset = Some(offset);
        self
    }

    /// `FOR UPDATE` is a no-op on embedded SQLite — preserved for
    /// cross-backend ergonomics. See [`FindMany::for_update`].
    pub fn for_update(self) -> Self {
        self
    }

    pub fn run(self) -> Result<Vec<cratestack_sql::Projection<M>>, RusqliteError>
    where
        M: crate::FromPartialRusqliteRow,
    {
        let dialect = SqliteDialect;
        let (sql, binds) = build_partial_select(
            &dialect,
            self.descriptor,
            &self.selected,
            &self.filters,
            &self.order_by,
            self.limit,
            self.offset,
        );
        let selected = self.selected;
        self.runtime.with_connection(|conn| {
            let mut stmt = conn.prepare(&sql)?;
            let bind_iter = binds.iter().map(SqlValueParam);
            let rows = stmt
                .query_map(params_from_iter(bind_iter), |row| {
                    M::from_partial_rusqlite_row(row, &selected)
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows
                .into_iter()
                .map(|value| cratestack_sql::Projection {
                    value,
                    selected: selected.clone(),
                })
                .collect())
        })
    }

    /// Cross-backend `run_in_tx` shape. See [`FindMany::run_in_tx`]
    /// for embedded-vs-server semantics.
    pub fn run_in_tx(
        self,
        conn: &rusqlite::Connection,
    ) -> Result<Vec<cratestack_sql::Projection<M>>, RusqliteError>
    where
        M: crate::FromPartialRusqliteRow,
    {
        let dialect = SqliteDialect;
        let (sql, binds) = build_partial_select(
            &dialect,
            self.descriptor,
            &self.selected,
            &self.filters,
            &self.order_by,
            self.limit,
            self.offset,
        );
        let selected = self.selected;
        let mut stmt = conn.prepare(&sql)?;
        let bind_iter = binds.iter().map(SqlValueParam);
        let rows = stmt
            .query_map(params_from_iter(bind_iter), |row| {
                M::from_partial_rusqlite_row(row, &selected)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .map(|value| cratestack_sql::Projection {
                value,
                selected: selected.clone(),
            })
            .collect())
    }
}
