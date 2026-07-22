//! `DeleteRecord` — single-row DELETE (soft or hard) with policy +
//! audit + event fan-out. For a hard delete, the `RETURNING` row IS
//! the pre-delete state, so it doubles as the audit "before" snapshot
//! and "after" stays `None` (the row no longer exists). For a soft
//! delete, `delete_returning_record` actually runs an `UPDATE ...
//! RETURNING`, so that row is the *post*-tombstone state — it's
//! captured as "after", and "before" comes from a separate row-locked
//! fetch taken ahead of the mutation, mirroring how `update.rs` splits
//! its own before/after snapshots.

use cratestack_core::{AuditOperation, CoolContext, CoolError, ModelEventKind};

use crate::audit::{build_audit_event, enqueue_audit_event, ensure_audit_table, fetch_for_audit};
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
        PK: Send + Clone + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        let emits_event = self.descriptor.emits(ModelEventKind::Deleted);
        let audit_enabled = self.descriptor.audit_enabled;
        let soft_delete = self.descriptor.soft_delete_column.is_some();
        if emits_event {
            ensure_event_outbox_table(&mut **tx).await?;
        }
        if audit_enabled {
            ensure_audit_table(self.runtime).await?;
        }
        // Soft delete is an UPDATE under the hood, so its RETURNING
        // row is the post-tombstone state — the pre-delete "before"
        // snapshot has to come from a separate row-locked read taken
        // ahead of the mutation.
        let before_record = if audit_enabled && soft_delete {
            fetch_for_audit(&mut **tx, self.descriptor, self.id.clone()).await?
        } else {
            None
        };
        let before_snapshot = before_record
            .as_ref()
            .and_then(|m| serde_json::to_value(m).ok());
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
            let (before, after) = if soft_delete {
                (before_snapshot, serde_json::to_value(&record).ok())
            } else {
                (serde_json::to_value(&record).ok(), None)
            };
            let event =
                build_audit_event(self.descriptor, AuditOperation::Delete, before, after, ctx);
            enqueue_audit_event(&mut **tx, &event).await?;
        }
        Ok(record)
    }

    pub async fn run(self, ctx: &CoolContext) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + Clone + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        let emits_event = self.descriptor.emits(ModelEventKind::Deleted);
        let audit_enabled = self.descriptor.audit_enabled;
        let soft_delete = self.descriptor.soft_delete_column.is_some();
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
                ensure_audit_table(self.runtime).await?;
            }

            let before_record = if audit_enabled && soft_delete {
                fetch_for_audit(&mut *tx, self.descriptor, self.id.clone()).await?
            } else {
                None
            };
            let before_snapshot = before_record
                .as_ref()
                .and_then(|m| serde_json::to_value(m).ok());
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
                let (before, after) = if soft_delete {
                    (before_snapshot, serde_json::to_value(&record).ok())
                } else {
                    (serde_json::to_value(&record).ok(), None)
                };
                let event =
                    build_audit_event(self.descriptor, AuditOperation::Delete, before, after, ctx);
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
