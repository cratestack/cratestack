//! Context-bound wrapper around [`crate::FindManyWith`] —
//! `find_many().include(...)` resolved against a `CoolContext`.

use cratestack_core::{CoolContext, CoolError};

use crate::{Filter, FilterExpr, OrderClause, sqlx};

#[derive(Debug, Clone)]
pub struct ScopedFindManyWith<'a, M: 'static, PK: 'static, Rel: 'static, RelPK: 'static> {
    pub(super) request: crate::FindManyWith<'a, M, PK, Rel, RelPK>,
    pub(super) ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static, Rel: 'static, RelPK: 'static>
    ScopedFindManyWith<'a, M, PK, Rel, RelPK>
{
    pub(super) fn new(
        request: crate::FindManyWith<'a, M, PK, Rel, RelPK>,
        ctx: CoolContext,
    ) -> Self {
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

    /// See [`crate::FindManyWith::for_update`].
    pub fn for_update(mut self) -> Self {
        self.request = self.request.for_update();
        self
    }

    pub async fn run(self) -> Result<Vec<(M, Option<Rel>)>, CoolError>
    where
        M: Clone,
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
        Rel: Clone,
        for<'r> Rel: Send
            + Unpin
            + sqlx::FromRow<'r, sqlx::postgres::PgRow>
            + cratestack_sql::ModelPrimaryKey<RelPK>,
        RelPK: Send
            + Clone
            + std::cmp::Eq
            + std::hash::Hash
            + cratestack_sql::IntoSqlValue
            + sqlx::Type<sqlx::Postgres>
            + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run(&self.ctx).await
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    ) -> Result<Vec<(M, Option<Rel>)>, CoolError>
    where
        M: Clone,
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
        Rel: Clone,
        for<'r> Rel: Send
            + Unpin
            + sqlx::FromRow<'r, sqlx::postgres::PgRow>
            + cratestack_sql::ModelPrimaryKey<RelPK>,
        RelPK: Send
            + Clone
            + std::cmp::Eq
            + std::hash::Hash
            + cratestack_sql::IntoSqlValue
            + sqlx::Type<sqlx::Postgres>
            + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run_in_tx(tx, &self.ctx).await
    }
}
