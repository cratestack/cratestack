//! `FindMany` — fluent SELECT builder with filter/order/limit/offset and
//! transaction-scoped variants.

use cratestack_sql::{Filter, FilterExpr, OrderClause, ReadSource, SqliteDialect};
use rusqlite::params_from_iter;

use crate::{
    FromRusqliteRow, RusqliteError, RusqliteRuntime, SqlValueParam, render::render_select,
};

use super::find_many_with::FindManyWith;
use super::projected_find_many::ProjectedFindMany;

pub struct FindMany<'a, M: 'static, PK: 'static> {
    pub(super) runtime: &'a RusqliteRuntime,
    pub(super) descriptor: &'static dyn ReadSource<M, PK>,
    pub(super) filters: Vec<FilterExpr>,
    pub(super) order_by: Vec<OrderClause>,
    pub(super) limit: Option<i64>,
    pub(super) offset: Option<i64>,
}

impl<'a, M: 'static, PK: 'static> FindMany<'a, M, PK> {
    pub fn where_(mut self, filter: Filter) -> Self {
        self.filters.push(FilterExpr::from(filter));
        self
    }

    pub fn where_expr(mut self, filter: FilterExpr) -> Self {
        self.filters.push(filter);
        self
    }

    /// Conditionally append a filter — `None` is a no-op. Mirrors
    /// the sqlx delegate's `where_optional` so cross-backend code can
    /// stay backend-agnostic when handling optional query
    /// parameters.
    pub fn where_optional<F>(mut self, filter: Option<F>) -> Self
    where
        F: Into<FilterExpr>,
    {
        if let Some(filter) = filter {
            self.filters.push(filter.into());
        }
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

    /// API-compat no-op. SQLite has no `SELECT ... FOR UPDATE` — its
    /// transaction model uses whole-database write locks (`BEGIN IMMEDIATE`),
    /// which already give the serialization guarantees the server-side
    /// `FOR UPDATE` is reaching for. Kept on the embedded delegate so
    /// schemas can compile and tests can share code across backends.
    pub fn for_update(self) -> Self {
        self
    }

    pub fn preview_sql(&self) -> String {
        let dialect = SqliteDialect;
        let (sql, _) = render_select(
            &dialect,
            self.descriptor,
            &self.filters,
            &self.order_by,
            self.limit,
            self.offset,
        );
        sql
    }

    pub fn run(self) -> Result<Vec<M>, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        let dialect = SqliteDialect;
        let (sql, binds) = render_select(
            &dialect,
            self.descriptor,
            &self.filters,
            &self.order_by,
            self.limit,
            self.offset,
        );
        self.runtime.with_connection(|conn| {
            let mut stmt = conn.prepare(&sql)?;
            let bind_iter = binds.iter().map(SqlValueParam);
            let rows = stmt
                .query_map(params_from_iter(bind_iter), |row| M::from_rusqlite_row(row))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// Run against a caller-supplied connection (typically the active
    /// transaction's connection, via `&mut *tx`). Mirrors the sqlx
    /// `run_in_tx` shape for cross-backend ergonomics; on rusqlite this
    /// is just the same query executed against the provided connection
    /// instead of the runtime's mutex-guarded one.
    pub fn run_in_tx(self, conn: &rusqlite::Connection) -> Result<Vec<M>, RusqliteError>
    where
        M: FromRusqliteRow,
    {
        let dialect = SqliteDialect;
        let (sql, binds) = render_select(
            &dialect,
            self.descriptor,
            &self.filters,
            &self.order_by,
            self.limit,
            self.offset,
        );
        let mut stmt = conn.prepare(&sql)?;
        let bind_iter = binds.iter().map(SqlValueParam);
        let rows = stmt
            .query_map(params_from_iter(bind_iter), |row| M::from_rusqlite_row(row))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Side-load a to-one relation. See [`cratestack_sqlx::FindMany::include`]
    /// for the rationale; the embedded mirror uses the same two-step
    /// approach (parent query + IN-list child query, merge in memory).
    pub fn include<Rel: 'static, RelPK: 'static>(
        self,
        relation: cratestack_sql::RelationInclude<M, Rel, RelPK>,
    ) -> FindManyWith<'a, M, PK, Rel, RelPK> {
        FindManyWith {
            parent: self,
            relation,
        }
    }

    pub fn select<I, C>(self, columns: I) -> ProjectedFindMany<'a, M, PK>
    where
        I: IntoIterator<Item = C>,
        C: cratestack_sql::IntoColumnName,
    {
        ProjectedFindMany {
            runtime: self.runtime,
            descriptor: self.descriptor,
            filters: self.filters,
            order_by: self.order_by,
            limit: self.limit,
            offset: self.offset,
            selected: columns
                .into_iter()
                .map(cratestack_sql::IntoColumnName::into_column_name)
                .collect(),
        }
    }
}
