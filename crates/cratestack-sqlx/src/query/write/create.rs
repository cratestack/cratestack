//! `CreateRecord` — single-row INSERT with policy + audit + event
//! fan-out. `run()` opens its own tx only when audit/event capture is
//! enabled; otherwise it goes straight against the pool.

use cratestack_core::{AuditOperation, CoolContext, CoolError, ModelEventKind};

use crate::audit::{build_audit_event, enqueue_audit_event, ensure_audit_table};
use crate::descriptor::{enqueue_event_outbox, ensure_event_outbox_table};
use crate::{CreateModelInput, ModelDescriptor, SqlxRuntime, sqlx};

use super::create_exec::create_record_with_executor;

#[derive(Debug, Clone)]
pub struct CreateRecord<'a, M: 'static, PK: 'static, I> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) input: I,
}

impl<'a, M: 'static, PK: 'static, I> CreateRecord<'a, M, PK, I>
where
    I: CreateModelInput<M>,
{
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

        format!(
            "INSERT INTO {} ({}) VALUES ({}) RETURNING {}",
            self.descriptor.table_name,
            columns,
            placeholders,
            self.descriptor.select_projection(),
        )
    }

    /// Like [`Self::run`] but participates in a caller-supplied
    /// transaction. The insert + outbox + audit writes all happen
    /// inside `tx`; caller commits. Event outbox is *not* drained —
    /// the outbox row isn't visible to the drain worker until commit.
    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
        ctx: &CoolContext,
    ) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    {
        let emits_event = self.descriptor.emits(ModelEventKind::Created);
        let audit_enabled = self.descriptor.audit_enabled;
        if emits_event {
            ensure_event_outbox_table(&mut **tx).await?;
        }
        if audit_enabled {
            ensure_audit_table(self.runtime.pool()).await?;
        }
        let record = create_record_with_executor(
            &mut **tx,
            self.runtime.pool(),
            self.descriptor,
            self.input,
            ctx,
        )
        .await?;
        if emits_event {
            enqueue_event_outbox(
                &mut **tx,
                self.descriptor.schema_name,
                ModelEventKind::Created,
                &record,
            )
            .await?;
        }
        if audit_enabled {
            let after = serde_json::to_value(&record).ok();
            let event =
                build_audit_event(self.descriptor, AuditOperation::Create, None, after, ctx);
            enqueue_audit_event(&mut **tx, &event).await?;
        }
        Ok(record)
    }

    pub async fn run(self, ctx: &CoolContext) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    {
        let emits_event = self.descriptor.emits(ModelEventKind::Created);
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
            let record = create_record_with_executor(
                &mut *tx,
                self.runtime.pool(),
                self.descriptor,
                self.input,
                ctx,
            )
            .await?;
            if emits_event {
                enqueue_event_outbox(
                    &mut *tx,
                    self.descriptor.schema_name,
                    ModelEventKind::Created,
                    &record,
                )
                .await?;
            }
            if audit_enabled {
                let after = serde_json::to_value(&record).ok();
                let event = build_audit_event(
                    self.descriptor,
                    AuditOperation::Create,
                    None,
                    after,
                    ctx,
                );
                enqueue_audit_event(&mut *tx, &event).await?;
            }
            tx.commit()
                .await
                .map_err(|error| CoolError::Database(error.to_string()))?;
            record
        } else {
            create_record_with_executor(
                self.runtime.pool(),
                self.runtime.pool(),
                self.descriptor,
                self.input,
                ctx,
            )
            .await?
        };

        if emits_event {
            let _ = self.runtime.drain_event_outbox().await;
        }

        Ok(record)
    }
}
