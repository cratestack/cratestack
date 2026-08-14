//! Single-row + predicate-driven bulk DELETE wrappers bound to a
//! `CratestackContext`.

use cratestack_core::{CratestackContext, CratestackError};

use crate::audit::RunInTxOutcome;
use crate::{DeleteMany, DeleteRecord, Filter, FilterExpr, sqlx};

#[derive(Debug, Clone)]
pub struct ScopedDeleteRecord<'a, M: 'static, PK: 'static> {
    request: DeleteRecord<'a, M, PK>,
    ctx: CratestackContext,
}

impl<'a, M: 'static, PK: 'static> ScopedDeleteRecord<'a, M, PK> {
    pub(super) fn new(request: DeleteRecord<'a, M, PK>, ctx: CratestackContext) -> Self {
        Self { request, ctx }
    }

    pub fn preview_sql(&self) -> String {
        self.request.preview_sql()
    }

    /// Attach an expected version for optimistic locking. See
    /// [`DeleteRecord::if_match`].
    pub fn if_match(mut self, expected: i64) -> Self {
        self.request = self.request.if_match(expected);
        self
    }

    pub async fn run(self) -> Result<M, CratestackError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + Clone + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run(&self.ctx).await
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    ) -> Result<RunInTxOutcome<M>, CratestackError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + Clone + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run_in_tx(tx, &self.ctx).await
    }
}

#[derive(Debug, Clone)]
pub struct ScopedDeleteMany<'a, M: 'static, PK: 'static> {
    request: DeleteMany<'a, M, PK>,
    ctx: CratestackContext,
}

impl<'a, M: 'static, PK: 'static> ScopedDeleteMany<'a, M, PK> {
    pub(super) fn new(request: DeleteMany<'a, M, PK>, ctx: CratestackContext) -> Self {
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

    pub async fn run(self) -> Result<cratestack_core::BatchSummary, CratestackError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    {
        self.request.run(&self.ctx).await
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    ) -> Result<RunInTxOutcome<cratestack_core::BatchSummary>, CratestackError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    {
        self.request.run_in_tx(tx, &self.ctx).await
    }
}
