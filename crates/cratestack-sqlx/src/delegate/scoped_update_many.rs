//! `update_many().where_(...).set(...)` wrappers — predicate-driven
//! bulk UPDATE bound to a `CoolContext`.

use cratestack_core::{CoolContext, CoolError};

use crate::{Filter, FilterExpr, UpdateMany, UpdateManySet, UpdateModelInput, sqlx};

#[derive(Debug, Clone)]
pub struct ScopedUpdateMany<'a, M: 'static, PK: 'static> {
    request: UpdateMany<'a, M, PK>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static> ScopedUpdateMany<'a, M, PK> {
    pub(super) fn new(request: UpdateMany<'a, M, PK>, ctx: CoolContext) -> Self {
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

    /// See [`UpdateMany::where_optional`].
    pub fn where_optional<F>(mut self, filter: Option<F>) -> Self
    where
        F: Into<FilterExpr>,
    {
        self.request = self.request.where_optional(filter);
        self
    }

    pub fn set<I>(self, input: I) -> ScopedUpdateManySet<'a, M, PK, I> {
        ScopedUpdateManySet {
            request: self.request.set(input),
            ctx: self.ctx,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScopedUpdateManySet<'a, M: 'static, PK: 'static, I> {
    request: UpdateManySet<'a, M, PK, I>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static, I> ScopedUpdateManySet<'a, M, PK, I>
where
    I: UpdateModelInput<M>,
{
    pub fn preview_sql(&self) -> String {
        self.request.preview_sql()
    }

    pub async fn run(self) -> Result<cratestack_core::BatchSummary, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    {
        self.request.run(&self.ctx).await
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    ) -> Result<cratestack_core::BatchSummary, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    {
        self.request.run_in_tx(tx, &self.ctx).await
    }
}
