//! `CreateRecord` — single-row INSERT with policy + audit + event
//! fan-out. `run()` opens its own tx only when audit/event capture is
//! enabled; otherwise it goes straight against the pool.

use cratestack_core::{AuditOperation, CoolContext, CoolError, ModelEventKind};

use crate::audit::{
    build_audit_event, dispatch_audit_sink, enqueue_audit_event, ensure_audit_table,
};
use crate::descriptor::{enqueue_event_outbox, ensure_event_outbox_table};
use crate::{CreateModelInput, ModelDescriptor, SqlxRuntime, cool_error_from_sqlx, sqlx};

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
    /// Same reasoning applies to the `AuditSink` fan-out (cratestack#473):
    /// it does not run here either, since this function has no
    /// visibility into when — or whether — the caller commits `tx`.
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
            ensure_audit_table(self.runtime).await?;
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
        let mut audit_event = None;
        let record = if needs_tx {
            let mut tx = self
                .runtime
                .pool()
                .begin()
                .await
                .map_err(cool_error_from_sqlx)?;
            if emits_event {
                ensure_event_outbox_table(&mut *tx).await?;
            }
            if audit_enabled {
                ensure_audit_table(self.runtime).await?;
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
                let event =
                    build_audit_event(self.descriptor, AuditOperation::Create, None, after, ctx);
                enqueue_audit_event(&mut *tx, &event).await?;
                audit_event = Some(event);
            }
            tx.commit().await.map_err(cool_error_from_sqlx)?;
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
        if let Some(event) = &audit_event {
            dispatch_audit_sink(self.runtime, std::slice::from_ref(event)).await;
        }

        Ok(record)
    }
}
