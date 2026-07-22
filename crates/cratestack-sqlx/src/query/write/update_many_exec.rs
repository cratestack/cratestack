//! Core body of `update_many`: build the bulk UPDATE with RETURNING,
//! fan-out event + audit, return a `BatchSummary { total, ok, err: 0 }`.

use cratestack_core::{AuditOperation, BatchSummary, CoolContext, CoolError, ModelEventKind};

use crate::audit::{build_audit_event, enqueue_audit_event, ensure_audit_table};
use crate::descriptor::{enqueue_event_outbox, ensure_event_outbox_table};
use crate::query::support::{push_action_policy_query, push_bind_value, push_filter_query};
use crate::{FilterExpr, ModelDescriptor, SqlxRuntime, UpdateModelInput, sqlx};

pub(super) async fn run_update_many_in_tx<'tx, M, PK, I>(
    tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    runtime: &SqlxRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    filters: &[FilterExpr],
    input: I,
    ctx: &CoolContext,
) -> Result<(BatchSummary, bool), CoolError>
where
    I: UpdateModelInput<M>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
{
    if filters.is_empty() {
        return Err(CoolError::Validation(
            "update_many requires at least one filter — refusing table-wide update".to_owned(),
        ));
    }
    input.validate()?;
    let values = input.sql_values();
    if values.is_empty() {
        return Err(CoolError::Validation(
            "update input must contain at least one changed column".to_owned(),
        ));
    }

    let emits_event = descriptor.emits(ModelEventKind::Updated);
    let audit_enabled = descriptor.audit_enabled;
    if emits_event {
        ensure_event_outbox_table(&mut **tx).await?;
    }
    if audit_enabled {
        ensure_audit_table(runtime).await?;
    }

    // We always read back the mutated rows via RETURNING so
    // audit/event fan-out works and `BatchSummary.ok` is accurate.
    let version_column = descriptor.version_column;
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("UPDATE ");
    query.push(descriptor.table_name).push(" SET ");
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            query.push(", ");
        }
        query.push(value.column).push(" = ");
        push_bind_value(&mut query, &value.value);
    }
    if let Some(version_col) = version_column {
        query
            .push(", ")
            .push(version_col)
            .push(" = ")
            .push(version_col)
            .push(" + 1");
    }

    query.push(" WHERE ");
    let mut wrote = false;
    if let Some(col) = descriptor.soft_delete_column {
        query.push(col).push(" IS NULL");
        wrote = true;
    }
    if wrote {
        query.push(" AND ");
    }
    query.push("(");
    push_filter_query(&mut query, filters);
    query.push(") AND ");
    push_action_policy_query(
        &mut query,
        descriptor.update_allow_policies,
        descriptor.update_deny_policies,
        ctx,
    );
    query
        .push(" RETURNING ")
        .push(descriptor.select_projection());

    let updated: Vec<M> = query
        .build_query_as::<M>()
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| CoolError::Database(error.to_string()))?;

    for record in &updated {
        if emits_event {
            enqueue_event_outbox(
                &mut **tx,
                descriptor.schema_name,
                ModelEventKind::Updated,
                record,
            )
            .await?;
        }
        if audit_enabled {
            // No before-snapshot: capturing one would require a
            // SELECT FOR UPDATE of every matched row, doubling
            // round-trips. The audit row records the after state +
            // the operation kind; consumers wanting a diff compare
            // against the previous audit row for the same PK.
            let after = serde_json::to_value(record).ok();
            let event = build_audit_event(descriptor, AuditOperation::Update, None, after, ctx);
            enqueue_audit_event(&mut **tx, &event).await?;
        }
    }

    let total = updated.len();
    Ok((
        BatchSummary {
            total,
            ok: total,
            err: 0,
        },
        emits_event,
    ))
}
