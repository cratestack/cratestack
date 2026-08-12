//! `UpsertRecordDoNothing` — the `.upsert(..).do_nothing()` builder
//! (cratestack#487). Produced by [`super::upsert::UpsertRecord::do_nothing`];
//! see that method's doc comment for why this is a distinct type
//! rather than a flag on `UpsertRecord` itself.

use cratestack_core::{CoolContext, CoolError};

use crate::audit::{RunInTxOutcome, dispatch_audit_sink};
use crate::{
    ConflictTarget, ModelDescriptor, SqlxRuntime, UpsertModelInput, cool_error_from_sqlx, sqlx,
};

use super::upsert_do_nothing_exec::run_upsert_do_nothing_in_tx;
use super::upsert_outcome::UpsertOutcome;

#[derive(Debug, Clone)]
pub struct UpsertRecordDoNothing<'a, M: 'static, PK: 'static, I> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) input: I,
    pub(crate) conflict_target: ConflictTarget,
}

impl<'a, M: 'static, PK: 'static, I> UpsertRecordDoNothing<'a, M, PK, I>
where
    I: UpsertModelInput<M>,
{
    /// Choose the conflict target. See [`super::upsert::UpsertRecord::on_conflict`];
    /// works identically here, and can be called either before or
    /// after `.do_nothing()`.
    pub fn on_conflict(mut self, target: ConflictTarget) -> Self {
        self.conflict_target = target;
        self
    }

    /// Render an approximate SQL preview of the insert-branch
    /// statement. The actual call wraps a `SELECT ... FOR UPDATE`
    /// probe around it and may perform a fallback `SELECT` on a lost
    /// race — see [`UpsertOutcome`] for the full sequencing.
    pub fn preview_sql(&self) -> String {
        let values = self.input.sql_values();
        let placeholders = (1..=values.len())
            .map(|index| format!("${index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let columns = values
            .iter()
            .map(|value| value.column)
            .collect::<Vec<_>>()
            .join(", ");
        let conflict_tuple = match self.conflict_target {
            ConflictTarget::PrimaryKey => self.descriptor.primary_key.to_owned(),
            ConflictTarget::Columns(cols) => cols.join(", "),
        };

        format!(
            "INSERT INTO {table} ({columns}) VALUES ({placeholders}) \
             ON CONFLICT ({conflict_tuple}) DO NOTHING \
             RETURNING {projection}",
            table = self.descriptor.table_name,
            projection = self.descriptor.select_projection(),
        )
    }

    pub async fn run(self, ctx: &CoolContext) -> Result<UpsertOutcome<M>, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        let runtime = self.runtime;
        let mut tx = runtime.pool().begin().await.map_err(cool_error_from_sqlx)?;
        let (outcome, emits_event, audit_event) = run_upsert_do_nothing_in_tx(
            &mut tx,
            runtime,
            self.descriptor,
            self.input,
            self.conflict_target,
            ctx,
        )
        .await?;
        tx.commit().await.map_err(cool_error_from_sqlx)?;
        if emits_event {
            let _ = runtime.drain_event_outbox().await;
        }
        if let Some(event) = &audit_event {
            dispatch_audit_sink(runtime, std::slice::from_ref(event)).await;
        }
        Ok(outcome)
    }

    /// Like [`Self::run`] but participates in a caller-supplied
    /// transaction. The conflict probe (and, on the insert branch, the
    /// `ON CONFLICT DO NOTHING`) run against `tx`, so any row lock is
    /// held until the caller commits. Neither the event outbox drain
    /// nor the `AuditSink` fan-out happens here — see `create.rs`'s
    /// `run_in_tx` doc comment for the full contract and how a caller
    /// opts into both after their own commit.
    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
        ctx: &CoolContext,
    ) -> Result<RunInTxOutcome<UpsertOutcome<M>>, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        let (outcome, _emits_event, audit_event) = run_upsert_do_nothing_in_tx(
            tx,
            self.runtime,
            self.descriptor,
            self.input,
            self.conflict_target,
            ctx,
        )
        .await?;
        Ok(RunInTxOutcome::new(
            outcome,
            audit_event.into_iter().collect(),
        ))
    }
}
