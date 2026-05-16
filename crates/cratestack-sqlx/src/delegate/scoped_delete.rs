//! Single-row + predicate-driven bulk DELETE wrappers bound to a
//! `CoolContext`.

use cratestack_core::{CoolContext, CoolError};

use crate::{DeleteMany, DeleteRecord, Filter, FilterExpr, sqlx};

#[derive(Debug, Clone)]
pub struct ScopedDeleteRecord<'a, M: 'static, PK: 'static> {
    request: DeleteRecord<'a, M, PK>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static> ScopedDeleteRecord<'a, M, PK> {
    pub(super) fn new(request: DeleteRecord<'a, M, PK>, ctx: CoolContext) -> Self {
        Self { request, ctx }
    }

    pub fn preview_sql(&self) -> String {
        self.request.preview_sql()
    }

    pub async fn run(self) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run(&self.ctx).await
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    ) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run_in_tx(tx, &self.ctx).await
    }
}

#[derive(Debug, Clone)]
pub struct ScopedDeleteMany<'a, M: 'static, PK: 'static> {
    request: DeleteMany<'a, M, PK>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static> ScopedDeleteMany<'a, M, PK> {
    pub(super) fn new(request: DeleteMany<'a, M, PK>, ctx: CoolContext) -> Self {
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

    /// See [`DeleteMany::where_optional`].
    pub fn where_optional<F>(mut self, filter: Option<F>) -> Self
    where
        F: Into<FilterExpr>,
    {
        self.request = self.request.where_optional(filter);
        self
    }

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
