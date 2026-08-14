//! `batch_upsert` driver — dedupes inputs by PK (different shape from
//! `batch_update` because `UpsertModelInput` exposes a PK getter), then
//! fans out to [`super::upsert_item::run_upsert_item`].

use cratestack_core::{BatchResponse, CratestackContext, CratestackError, ModelEventKind};

use crate::audit::{dispatch_audit_sink, ensure_audit_table};
use crate::descriptor::ensure_event_outbox_table;
use crate::{ModelDescriptor, SqlValue, SqlxRuntime, UpsertModelInput, cool_error_from_sqlx, sqlx};

use super::upsert_item::run_upsert_item;
use super::validate::{reject_duplicate_sql_values, validate_batch_size};

#[derive(Debug, Clone)]
pub struct BatchUpsert<'a, M: 'static, PK: 'static, I> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) inputs: Vec<I>,
}

impl<'a, M: 'static, PK: 'static, I> BatchUpsert<'a, M, PK, I>
where
    I: UpsertModelInput<M>,
{
    pub async fn run(self, ctx: &CratestackContext) -> Result<BatchResponse<M>, CratestackError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        validate_batch_size(self.inputs.len())?;
        // Upsert dedup runs on the per-input primary key — keeps two
        // callers from both producing batches with the same key and
        // ending up with surprising "second write wins" semantics.
        let pks: Vec<SqlValue> = self
            .inputs
            .iter()
            .map(UpsertModelInput::primary_key_value)
            .collect();
        reject_duplicate_sql_values(&pks)?;
        if self.inputs.is_empty() {
            return Ok(BatchResponse::from_results(vec![]));
        }

        let emits_created = self.descriptor.emits(ModelEventKind::Created);
        let emits_updated = self.descriptor.emits(ModelEventKind::Updated);
        let audit_enabled = self.descriptor.audit_enabled;

        let mut tx = self
            .runtime
            .pool()
            .begin()
            .await
            .map_err(cool_error_from_sqlx)?;
        if emits_created || emits_updated {
            ensure_event_outbox_table(&mut *tx).await?;
        }
        if audit_enabled {
            ensure_audit_table(self.runtime).await?;
        }

        let mut per_item: Vec<Result<M, CratestackError>> = Vec::with_capacity(self.inputs.len());
        let mut audit_events = Vec::new();
        for input in self.inputs {
            let (outcome, audit_event) = run_upsert_item(
                &mut tx,
                self.runtime.pool(),
                self.descriptor,
                input,
                ctx,
                emits_created,
                emits_updated,
                audit_enabled,
            )
            .await?;
            per_item.push(outcome);
            audit_events.extend(audit_event);
        }

        tx.commit().await.map_err(cool_error_from_sqlx)?;

        if emits_created || emits_updated {
            let _ = self.runtime.drain_event_outbox().await;
        }
        dispatch_audit_sink(self.runtime, &audit_events).await;

        Ok(BatchResponse::from_results(per_item))
    }
}
