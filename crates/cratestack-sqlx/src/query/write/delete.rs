//! `DeleteRecord` — single-row DELETE (soft or hard) with policy +
//! audit + event fan-out. The RETURNING row doubles as the audit
//! "before" snapshot for hard deletes.

use cratestack_core::{AuditOperation, CoolContext, CoolError, ModelEventKind};

use crate::audit::{build_audit_event, enqueue_audit_event, ensure_audit_table};
use crate::descriptor::{enqueue_event_outbox, ensure_event_outbox_table};
use crate::{ModelDescriptor, SqlxRuntime, sqlx};

use super::delete_exec::delete_returning_record;

#[derive(Debug, Clone)]
pub struct DeleteRecord<'a, M: 'static, PK: 'static> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) id: PK,
}

impl<'a, M: 'static, PK: 'static> DeleteRecord<'a, M, PK> {
    pub fn preview_sql(&self) -> String {
        format!(
            "DELETE FROM {} WHERE {} = $1 RETURNING {}",
            self.descriptor.table_name,
            self.descriptor.primary_key,
            self.descriptor.select_projection(),
        )
    }

    /// Like [`Self::run`] but participates in a caller-supplied transaction.
    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
        ctx: &CoolContext,
    ) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        let emits_event = self.descriptor.emits(ModelEventKind::Deleted);
        let audit_enabled = self.descriptor.audit_enabled;
        if emits_event {
            ensure_event_outbox_table(&mut **tx).await?;
        }
        if audit_enabled {
            ensure_audit_table(self.runtime.pool()).await?;
        }
        let record = delete_returning_record(&mut **tx, self.descriptor, self.id, ctx).await?;
        if emits_event {
            enqueue_event_outbox(
                &mut **tx,
                self.descriptor.schema_name,
                ModelEventKind::Deleted,
                &record,
            )
            .await?;
        }
        if audit_enabled {
            let before = serde_json::to_value(&record).ok();
            let event =
                build_audit_event(self.descriptor, AuditOperation::Delete, before, None, ctx);
            enqueue_audit_event(&mut **tx, &event).await?;
        }
        Ok(record)
    }

    pub async fn run(self, ctx: &CoolContext) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        let emits_event = self.descriptor.emits(ModelEventKind::Deleted);
        let audit_enabled = self.descriptor.audit_enabled;
        let needs_tx = emits_event || audit_enabled;
        let record = if needs_tx {
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
                ensure_audit_table(self.runtime.pool()).await?;
            }

            let record = delete_returning_record(&mut *tx, self.descriptor, self.id, ctx).await?;
            if emits_event {
                enqueue_event_outbox(
                    &mut *tx,
                    self.descriptor.schema_name,
                    ModelEventKind::Deleted,
                    &record,
                )
                .await?;
            }
            if audit_enabled {
                // DELETE ... RETURNING yields the row's pre-delete
                // state, so it doubles as the audit `before` snapshot.
                let before = serde_json::to_value(&record).ok();
                let event = build_audit_event(
                    self.descriptor,
                    AuditOperation::Delete,
                    before,
                    None,
                    ctx,
                );
                enqueue_audit_event(&mut *tx, &event).await?;
            }
            tx.commit()
                .await
                .map_err(|error| CoolError::Database(error.to_string()))?;
            record
        } else {
            delete_returning_record(self.runtime.pool(), self.descriptor, self.id, ctx).await?
        };

        if emits_event {
            let _ = self.runtime.drain_event_outbox().await;
        }

        Ok(record)
    }
}
