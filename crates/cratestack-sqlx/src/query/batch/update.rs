//! `batch_update` driver — opens the outer tx, fans the items out to
//! [`super::update_item::run_update_item`] (one savepoint per item),
//! commits, drains the outbox.

use std::hash::Hash;

use cratestack_core::{BatchResponse, CratestackContext, CratestackError, ModelEventKind};

use crate::audit::{dispatch_audit_sink, ensure_audit_table};
use crate::descriptor::ensure_event_outbox_table;
use crate::{ModelDescriptor, SqlxRuntime, UpdateModelInput, cratestack_error_from_sqlx, sqlx};

use super::update_item::run_update_item;
use super::validate::{reject_duplicate_pks, validate_batch_size};

/// One per-item update: `(id, patch, optional expected version)`.
pub type BatchUpdateItem<PK, I> = (PK, I, Option<i64>);

#[derive(Debug, Clone)]
pub struct BatchUpdate<'a, M: 'static, PK: 'static, I> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) items: Vec<BatchUpdateItem<PK, I>>,
}

impl<'a, M: 'static, PK: 'static, I> BatchUpdate<'a, M, PK, I>
where
    I: UpdateModelInput<M> + Send,
{
    pub async fn run(self, ctx: &CratestackContext) -> Result<BatchResponse<M>, CratestackError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Clone
            + Eq
            + Hash
            + Send
            + sqlx::Type<sqlx::Postgres>
            + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        validate_batch_size(self.items.len())?;
        let ids: Vec<PK> = self.items.iter().map(|(id, _, _)| id.clone()).collect();
        reject_duplicate_pks(&ids)?;
        if self.items.is_empty() {
            return Ok(BatchResponse::from_results(vec![]));
        }

        let emits_event = self.descriptor.emits(ModelEventKind::Updated);
        let audit_enabled = self.descriptor.audit_enabled;

        let mut tx = self
            .runtime
            .pool()
            .begin()
            .await
            .map_err(cratestack_error_from_sqlx)?;
        if emits_event {
            ensure_event_outbox_table(&mut *tx).await?;
        }
        if audit_enabled {
            ensure_audit_table(self.runtime).await?;
        }

        let mut per_item: Vec<Result<M, CratestackError>> = Vec::with_capacity(self.items.len());
        let mut audit_events = Vec::new();
        for item in self.items {
            let (outcome, audit_event) = run_update_item(
                &mut tx,
                self.descriptor,
                item,
                ctx,
                emits_event,
                audit_enabled,
            )
            .await?;
            per_item.push(outcome);
            audit_events.extend(audit_event);
        }

        tx.commit().await.map_err(cratestack_error_from_sqlx)?;

        if emits_event {
            let _ = self.runtime.drain_event_outbox().await;
        }
        dispatch_audit_sink(self.runtime, &audit_events).await;

        Ok(BatchResponse::from_results(per_item))
    }
}
