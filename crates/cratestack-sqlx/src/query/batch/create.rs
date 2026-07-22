//! `batch_create` driver — opens the outer tx, fans the inputs out to
//! [`super::create_item::run_create_item`] (one savepoint per input),
//! commits, drains the outbox.

use cratestack_core::{BatchResponse, CoolContext, CoolError, ModelEventKind};

use crate::audit::ensure_audit_table;
use crate::descriptor::ensure_event_outbox_table;
use crate::{CreateModelInput, ModelDescriptor, SqlxRuntime, sqlx};

use super::create_item::run_create_item;
use super::validate::validate_batch_size;

#[derive(Debug, Clone)]
pub struct BatchCreate<'a, M: 'static, PK: 'static, I> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) inputs: Vec<I>,
}

impl<'a, M: 'static, PK: 'static, I> BatchCreate<'a, M, PK, I>
where
    I: CreateModelInput<M> + Send,
{
    pub async fn run(self, ctx: &CoolContext) -> Result<BatchResponse<M>, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    {
        validate_batch_size(self.inputs.len())?;
        // No PK dedup — `CreateModelInput` doesn't expose the PK
        // generically (and server-generated PKs make duplicates
        // impossible). Client-supplied PK collisions trip the DB
        // uniqueness constraint and surface as `CoolError::Database`.
        // The right primitive for idempotent client-PK ingestion is
        // `.batch_upsert(...)`.
        if self.inputs.is_empty() {
            return Ok(BatchResponse::from_results(vec![]));
        }

        let emits_event = self.descriptor.emits(ModelEventKind::Created);
        let audit_enabled = self.descriptor.audit_enabled;

        let mut tx = self
            .runtime
            .pool()
            .begin()
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
        if emits_event {
            ensure_event_outbox_table(&mut *tx).await?;
        }
        if audit_enabled {
            ensure_audit_table(self.runtime).await?;
        }

        let mut per_item: Vec<Result<M, CoolError>> = Vec::with_capacity(self.inputs.len());
        for input in self.inputs {
            let outcome = run_create_item(
                &mut tx,
                self.runtime.pool(),
                self.descriptor,
                input,
                ctx,
                emits_event,
                audit_enabled,
            )
            .await?;
            per_item.push(outcome);
        }

        tx.commit()
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;

        if emits_event {
            let _ = self.runtime.drain_event_outbox().await;
        }

        Ok(BatchResponse::from_results(per_item))
    }
}
