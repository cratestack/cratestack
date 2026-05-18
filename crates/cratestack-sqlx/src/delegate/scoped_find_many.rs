//! `ScopedFindMany` + the `select` / `include` exits to projected and
//! side-load variants.

use cratestack_core::{CoolContext, CoolError};

use crate::{Filter, FilterExpr, FindMany, OrderClause, sqlx};

use super::scoped_find_many_projected::ScopedProjectedFindMany;
use super::scoped_find_many_with::ScopedFindManyWith;

#[derive(Clone)]
pub struct ScopedFindMany<'a, M: 'static, PK: 'static> {
    pub(super) request: FindMany<'a, M, PK>,
    pub(super) ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static> ScopedFindMany<'a, M, PK> {
    pub(super) fn new(request: FindMany<'a, M, PK>, ctx: CoolContext) -> Self {
        Self { request, ctx }
    }

    pub fn where_(mut self, filter: Filter) -> Self {
        self.request = self.request.where_(filter);
        self
    }

    pub fn where_expr(mut self, filter: FilterExpr) -> Self {
        self.request = self.request.where_expr(filter);
        self
    }

    pub fn where_any(mut self, filters: impl IntoIterator<Item = FilterExpr>) -> Self {
        self.request = self.request.where_any(filters);
        self
    }

    /// See [`FindMany::where_optional`].
    pub fn where_optional<F>(mut self, filter: Option<F>) -> Self
    where
        F: Into<FilterExpr>,
    {
        self.request = self.request.where_optional(filter);
        self
    }

    pub fn order_by(mut self, clause: OrderClause) -> Self {
        self.request = self.request.order_by(clause);
        self
    }

    pub fn limit(mut self, limit: i64) -> Self {
        self.request = self.request.limit(limit);
        self
    }

    pub fn offset(mut self, offset: i64) -> Self {
        self.request = self.request.offset(offset);
        self
    }

    pub fn for_update(mut self) -> Self {
        self.request = self.request.for_update();
        self
    }

    pub fn preview_sql(&self) -> String {
        self.request.preview_sql()
    }

    pub fn preview_scoped_sql(&self) -> String {
        self.request.preview_scoped_sql(&self.ctx)
    }

    pub async fn run(self) -> Result<Vec<M>, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
    {
        self.request.run(&self.ctx).await
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    ) -> Result<Vec<M>, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
    {
        self.request.run_in_tx(tx, &self.ctx).await
    }

    /// See [`FindMany::include`].
    pub fn include<Rel: 'static, RelPK: 'static>(
        self,
        relation: cratestack_sql::RelationInclude<M, Rel, RelPK>,
    ) -> ScopedFindManyWith<'a, M, PK, Rel, RelPK> {
        ScopedFindManyWith::new(self.request.include(relation), self.ctx)
    }

    /// See [`FindMany::select`].
    pub fn select<I, C>(self, columns: I) -> ScopedProjectedFindMany<'a, M, PK>
    where
        I: IntoIterator<Item = C>,
        C: cratestack_sql::IntoColumnName,
    {
        ScopedProjectedFindMany::new(self.request.select(columns), self.ctx)
    }
}
