//! `.bind(ctx)`-scoped upsert wrappers, split out of `scoped_writes.rs`
//! (200-LoC ceiling) once `.do_nothing()` (cratestack#487) added a
//! second wrapper type alongside `ScopedUpsertRecord`.

use cratestack_core::{CratestackContext, CratestackError};

use crate::audit::RunInTxOutcome;
use crate::{UpsertModelInput, UpsertOutcome, UpsertRecord, UpsertRecordDoNothing, sqlx};

#[derive(Debug, Clone)]
pub struct ScopedUpsertRecord<'a, M: 'static, PK: 'static, I> {
    request: UpsertRecord<'a, M, PK, I>,
    ctx: CratestackContext,
}

impl<'a, M: 'static, PK: 'static, I> ScopedUpsertRecord<'a, M, PK, I> {
    pub(super) fn new(request: UpsertRecord<'a, M, PK, I>, ctx: CratestackContext) -> Self {
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

    /// See [`UpsertRecord::do_nothing`].
    pub fn do_nothing(self) -> ScopedUpsertRecordDoNothing<'a, M, PK, I> {
        ScopedUpsertRecordDoNothing {
            request: self.request.do_nothing(),
            ctx: self.ctx,
        }
    }

    pub fn preview_sql(&self) -> String {
        self.request.preview_sql()
    }

    pub async fn run(self) -> Result<M, CratestackError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run(&self.ctx).await
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    ) -> Result<RunInTxOutcome<M>, CratestackError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run_in_tx(tx, &self.ctx).await
    }
}

/// `.upsert(..).do_nothing()` bound to a `CratestackContext` via `.bind(ctx)`.
/// See [`UpsertRecordDoNothing`] for the run-time semantics; this is
/// purely a `ctx`-carrying wrapper, same relationship as
/// `ScopedUpsertRecord` has to `UpsertRecord`.
#[derive(Debug, Clone)]
pub struct ScopedUpsertRecordDoNothing<'a, M: 'static, PK: 'static, I> {
    request: UpsertRecordDoNothing<'a, M, PK, I>,
    ctx: CratestackContext,
}

impl<'a, M: 'static, PK: 'static, I> ScopedUpsertRecordDoNothing<'a, M, PK, I>
where
    I: UpsertModelInput<M>,
{
    /// See [`UpsertRecordDoNothing::on_conflict`].
    pub fn on_conflict(mut self, target: cratestack_sql::ConflictTarget) -> Self {
        self.request = self.request.on_conflict(target);
        self
    }

    pub fn preview_sql(&self) -> String {
        self.request.preview_sql()
    }

    pub async fn run(self) -> Result<UpsertOutcome<M>, CratestackError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run(&self.ctx).await
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    ) -> Result<RunInTxOutcome<UpsertOutcome<M>>, CratestackError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run_in_tx(tx, &self.ctx).await
    }
}
