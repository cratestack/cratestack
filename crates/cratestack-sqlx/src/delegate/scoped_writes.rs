//! Single-row write wrappers: create / update. Upsert (`ScopedUpsertRecord`
//! / `ScopedUpsertRecordDoNothing`) lives in `scoped_upsert.rs` — split
//! out once `.do_nothing()` (cratestack#487) pushed this file over the
//! 200-LoC ceiling.

use cratestack_core::{CratestackContext, CratestackError};

use crate::audit::RunInTxOutcome;
use crate::{
    CreateModelInput, CreateRecord, UpdateModelInput, UpdateRecord, UpdateRecordSet, sqlx,
};

#[derive(Debug, Clone)]
pub struct ScopedCreateRecord<'a, M: 'static, PK: 'static, I> {
    request: CreateRecord<'a, M, PK, I>,
    ctx: CratestackContext,
}

impl<'a, M: 'static, PK: 'static, I> ScopedCreateRecord<'a, M, PK, I> {
    pub(super) fn new(request: CreateRecord<'a, M, PK, I>, ctx: CratestackContext) -> Self {
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

    pub async fn run(self) -> Result<M, CratestackError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    {
        self.request.run(&self.ctx).await
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    ) -> Result<RunInTxOutcome<M>, CratestackError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    {
        self.request.run_in_tx(tx, &self.ctx).await
    }
}

#[derive(Debug, Clone)]
pub struct ScopedUpdateRecord<'a, M: 'static, PK: 'static> {
    request: UpdateRecord<'a, M, PK>,
    ctx: CratestackContext,
}

impl<'a, M: 'static, PK: 'static> ScopedUpdateRecord<'a, M, PK> {
    pub(super) fn new(request: UpdateRecord<'a, M, PK>, ctx: CratestackContext) -> Self {
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
    ctx: CratestackContext,
}

impl<'a, M: 'static, PK: 'static, I> ScopedUpdateRecordSet<'a, M, PK, I>
where
    I: UpdateModelInput<M>,
{
    pub fn preview_sql(&self) -> String {
        self.request.preview_sql()
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

    /// Attach an expected version for optimistic locking. See
    /// [`UpdateRecordSet::if_match`].
    pub fn if_match(mut self, expected: i64) -> Self {
        self.request = self.request.if_match(expected);
        self
    }
}
