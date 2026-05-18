//! Context-bound aggregate wrappers — `ScopedAggregate` dispatches to
//! `ScopedAggregateCount` / `ScopedAggregateColumn`.

use cratestack_core::{CoolContext, CoolError};

use crate::{Aggregate, AggregateColumn, AggregateCount, Filter, FilterExpr, sqlx};

#[derive(Clone)]
pub struct ScopedAggregate<'a, M: 'static, PK: 'static> {
    request: Aggregate<'a, M, PK>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static> ScopedAggregate<'a, M, PK> {
    pub(super) fn new(request: Aggregate<'a, M, PK>, ctx: CoolContext) -> Self {
        Self { request, ctx }
    }

    pub fn count(self) -> ScopedAggregateCount<'a, M, PK> {
        ScopedAggregateCount {
            request: self.request.count(),
            ctx: self.ctx,
        }
    }

    pub fn sum<C: cratestack_sql::IntoColumnName>(
        self,
        column: C,
    ) -> ScopedAggregateColumn<'a, M, PK> {
        ScopedAggregateColumn {
            request: self.request.sum(column),
            ctx: self.ctx,
        }
    }

    pub fn avg<C: cratestack_sql::IntoColumnName>(
        self,
        column: C,
    ) -> ScopedAggregateColumn<'a, M, PK> {
        ScopedAggregateColumn {
            request: self.request.avg(column),
            ctx: self.ctx,
        }
    }

    pub fn min<C: cratestack_sql::IntoColumnName>(
        self,
        column: C,
    ) -> ScopedAggregateColumn<'a, M, PK> {
        ScopedAggregateColumn {
            request: self.request.min(column),
            ctx: self.ctx,
        }
    }

    pub fn max<C: cratestack_sql::IntoColumnName>(
        self,
        column: C,
    ) -> ScopedAggregateColumn<'a, M, PK> {
        ScopedAggregateColumn {
            request: self.request.max(column),
            ctx: self.ctx,
        }
    }
}

#[derive(Clone)]
pub struct ScopedAggregateCount<'a, M: 'static, PK: 'static> {
    request: AggregateCount<'a, M, PK>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static> ScopedAggregateCount<'a, M, PK> {
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

    pub async fn run(self) -> Result<i64, CoolError> {
        self.request.run(&self.ctx).await
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    ) -> Result<i64, CoolError> {
        self.request.run_in_tx(tx, &self.ctx).await
    }
}

#[derive(Clone)]
pub struct ScopedAggregateColumn<'a, M: 'static, PK: 'static> {
    request: AggregateColumn<'a, M, PK>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static> ScopedAggregateColumn<'a, M, PK> {
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

    pub async fn run<T>(self) -> Result<Option<T>, CoolError>
    where
        T: Send + Unpin + for<'r> sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
    {
        self.request.run::<T>(&self.ctx).await
    }

    pub async fn run_in_tx<'tx, T>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    ) -> Result<Option<T>, CoolError>
    where
        T: Send + Unpin + for<'r> sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
    {
        self.request.run_in_tx::<T>(tx, &self.ctx).await
    }
}
