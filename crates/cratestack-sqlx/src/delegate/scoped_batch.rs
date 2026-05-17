//! Thin lifetime-and-context shims around the unscoped batch
//! builders. The shape mirrors the existing single-row scoped wrappers:
//! capture the request-bound `CoolContext` once at `.bind(ctx)` time,
//! thread it into `.run()` automatically.

use std::hash::Hash;

use cratestack_core::{BatchResponse, CoolContext, CoolError};

use crate::{
    BatchCreate, BatchDelete, BatchGet, BatchUpdate, BatchUpsert, CreateModelInput,
    UpdateModelInput, UpsertModelInput, sqlx,
};

#[derive(Debug, Clone)]
pub struct ScopedBatchGet<'a, M: 'static, PK: 'static> {
    request: BatchGet<'a, M, PK>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static> ScopedBatchGet<'a, M, PK> {
    pub(super) fn new(request: BatchGet<'a, M, PK>, ctx: CoolContext) -> Self {
        Self { request, ctx }
    }

    pub async fn run(self) -> Result<BatchResponse<M>, CoolError>
    where
        for<'r> M:
            Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + crate::ModelPrimaryKey<PK>,
        PK: Clone
            + Eq
            + Hash
            + Send
            + sqlx::Type<sqlx::Postgres>
            + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run(&self.ctx).await
    }
}

#[derive(Debug, Clone)]
pub struct ScopedBatchCreate<'a, M: 'static, PK: 'static, I> {
    request: BatchCreate<'a, M, PK, I>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static, I> ScopedBatchCreate<'a, M, PK, I> {
    pub(super) fn new(request: BatchCreate<'a, M, PK, I>, ctx: CoolContext) -> Self {
        Self { request, ctx }
    }
}

impl<'a, M: 'static, PK: 'static, I> ScopedBatchCreate<'a, M, PK, I>
where
    I: CreateModelInput<M> + Send,
{
    pub async fn run(self) -> Result<BatchResponse<M>, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    {
        self.request.run(&self.ctx).await
    }
}

#[derive(Debug, Clone)]
pub struct ScopedBatchUpdate<'a, M: 'static, PK: 'static, I> {
    request: BatchUpdate<'a, M, PK, I>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static, I> ScopedBatchUpdate<'a, M, PK, I> {
    pub(super) fn new(request: BatchUpdate<'a, M, PK, I>, ctx: CoolContext) -> Self {
        Self { request, ctx }
    }
}

impl<'a, M: 'static, PK: 'static, I> ScopedBatchUpdate<'a, M, PK, I>
where
    I: UpdateModelInput<M> + Send,
{
    pub async fn run(self) -> Result<BatchResponse<M>, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Clone
            + Eq
            + Hash
            + Send
            + sqlx::Type<sqlx::Postgres>
            + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run(&self.ctx).await
    }
}

#[derive(Debug, Clone)]
pub struct ScopedBatchDelete<'a, M: 'static, PK: 'static> {
    request: BatchDelete<'a, M, PK>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static> ScopedBatchDelete<'a, M, PK> {
    pub(super) fn new(request: BatchDelete<'a, M, PK>, ctx: CoolContext) -> Self {
        Self { request, ctx }
    }

    pub async fn run(self) -> Result<BatchResponse<M>, CoolError>
    where
        for<'r> M: Send
            + Unpin
            + sqlx::FromRow<'r, sqlx::postgres::PgRow>
            + crate::ModelPrimaryKey<PK>
            + serde::Serialize,
        PK: Clone
            + Eq
            + Hash
            + Send
            + sqlx::Type<sqlx::Postgres>
            + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run(&self.ctx).await
    }
}

#[derive(Debug, Clone)]
pub struct ScopedBatchUpsert<'a, M: 'static, PK: 'static, I> {
    request: BatchUpsert<'a, M, PK, I>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static, I> ScopedBatchUpsert<'a, M, PK, I> {
    pub(super) fn new(request: BatchUpsert<'a, M, PK, I>, ctx: CoolContext) -> Self {
        Self { request, ctx }
    }
}

impl<'a, M: 'static, PK: 'static, I> ScopedBatchUpsert<'a, M, PK, I>
where
    I: UpsertModelInput<M>,
{
    pub async fn run(self) -> Result<BatchResponse<M>, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run(&self.ctx).await
    }
}
