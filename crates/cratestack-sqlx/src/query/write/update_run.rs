//! The `UpdateRecordSet::run` body, factored out so the
//! [`super::update`] entry stays under the LoC budget. Opens its own
//! transaction when audit or event capture is needed; otherwise runs
//! straight against the pool.

use cratestack_core::{AuditOperation, CoolContext, CoolError, ModelEventKind};

use crate::audit::{build_audit_event, enqueue_audit_event, ensure_audit_table, fetch_for_audit};
use crate::descriptor::{enqueue_event_outbox, ensure_event_outbox_table};
use crate::{ModelDescriptor, SqlxRuntime, UpdateModelInput, sqlx};

use super::update_exec::update_record_with_executor;

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_update<'a, M, PK, I>(
    runtime: &'a SqlxRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
    input: I,
    if_match: Option<i64>,
    ctx: &CoolContext,
) -> Result<M, CoolError>
where
    I: UpdateModelInput<M>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    PK: Send + Clone + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
{
    if descriptor.version_column.is_some() && if_match.is_none() {
        return Err(CoolError::PreconditionFailed(
            "If-Match header required for versioned model".to_owned(),
        ));
    }
    let emits_event = descriptor.emits(ModelEventKind::Updated);
    let audit_enabled = descriptor.audit_enabled;
    let needs_tx = emits_event || audit_enabled;
    let record = if needs_tx {
        let mut tx = runtime
            .pool()
            .begin()
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
        if emits_event {
            ensure_event_outbox_table(&mut *tx).await?;
        }
        if audit_enabled {
            ensure_audit_table(runtime.pool()).await?;
        }
        // Before-snapshot under row lock so concurrent mutations
        // can't race.
        let before_record = if audit_enabled {
            fetch_for_audit(&mut *tx, descriptor, id.clone()).await?
        } else {
            None
        };
        let before_snapshot = before_record
            .as_ref()
            .and_then(|m| serde_json::to_value(m).ok());
        let record =
            update_record_with_executor(&mut *tx, runtime.pool(), descriptor, id, input, ctx, if_match)
                .await?;
        if emits_event {
            enqueue_event_outbox(
                &mut *tx,
                descriptor.schema_name,
                ModelEventKind::Updated,
                &record,
            )
            .await?;
        }
        if audit_enabled {
            let after = serde_json::to_value(&record).ok();
            let event = build_audit_event(
                descriptor,
                AuditOperation::Update,
                before_snapshot,
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
        update_record_with_executor(
            runtime.pool(),
            runtime.pool(),
            descriptor,
            id,
            input,
            ctx,
            if_match,
        )
        .await?
    };

    if emits_event {
        let _ = runtime.drain_event_outbox().await;
    }

    Ok(record)
}
