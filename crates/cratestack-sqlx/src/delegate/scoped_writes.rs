//! Single-row write wrappers: create / upsert / update.

use cratestack_core::{CoolContext, CoolError};

use crate::{
    CreateModelInput, CreateRecord, UpdateModelInput, UpdateRecord, UpdateRecordSet,
    UpsertModelInput, UpsertRecord, sqlx,
};

#[derive(Debug, Clone)]
pub struct ScopedCreateRecord<'a, M: 'static, PK: 'static, I> {
    request: CreateRecord<'a, M, PK, I>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static, I> ScopedCreateRecord<'a, M, PK, I> {
    pub(super) fn new(request: CreateRecord<'a, M, PK, I>, ctx: CoolContext) -> Self {
        Self { request, ctx }
    }
}

impl<'a, M: 'static, PK: 'static, I> ScopedCreateRecord<'a, M, PK, I>
where
    I: CreateModelInput<M>,
{
    pub fn preview_sql(&self) -> String {
        self.request.preview_sql()
    }

    pub async fn run(self) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    {
        self.request.run(&self.ctx).await
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    ) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    {
        self.request.run_in_tx(tx, &self.ctx).await
    }
}

#[derive(Debug, Clone)]
pub struct ScopedUpsertRecord<'a, M: 'static, PK: 'static, I> {
    request: UpsertRecord<'a, M, PK, I>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static, I> ScopedUpsertRecord<'a, M, PK, I> {
    pub(super) fn new(request: UpsertRecord<'a, M, PK, I>, ctx: CoolContext) -> Self {
        Self { request, ctx }
    }
}

impl<'a, M: 'static, PK: 'static, I> ScopedUpsertRecord<'a, M, PK, I>
where
    I: UpsertModelInput<M>,
{
    /// See [`UpsertRecord::on_conflict`].
    pub fn on_conflict(mut self, target: cratestack_sql::ConflictTarget) -> Self {
        self.request = self.request.on_conflict(target);
        self
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
pub struct ScopedUpdateRecord<'a, M: 'static, PK: 'static> {
    request: UpdateRecord<'a, M, PK>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static> ScopedUpdateRecord<'a, M, PK> {
    pub(super) fn new(request: UpdateRecord<'a, M, PK>, ctx: CoolContext) -> Self {
        Self { request, ctx }
    }

    pub fn set<I>(self, input: I) -> ScopedUpdateRecordSet<'a, M, PK, I> {
        ScopedUpdateRecordSet {
            request: self.request.set(input),
            ctx: self.ctx,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScopedUpdateRecordSet<'a, M: 'static, PK: 'static, I> {
    request: UpdateRecordSet<'a, M, PK, I>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static, I> ScopedUpdateRecordSet<'a, M, PK, I>
where
    I: UpdateModelInput<M>,
{
    pub fn preview_sql(&self) -> String {
        self.request.preview_sql()
    }

    pub async fn run(self) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + Clone + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run(&self.ctx).await
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    ) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + Clone + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run_in_tx(tx, &self.ctx).await
    }

    /// Attach an expected version for optimistic locking. See
    /// [`UpdateRecordSet::if_match`].
    pub fn if_match(mut self, expected: i64) -> Self {
        self.request = self.request.if_match(expected);
        self
    }
}
