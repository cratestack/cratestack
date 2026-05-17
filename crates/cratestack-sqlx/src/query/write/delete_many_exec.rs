//! Core body of `delete_many`: build soft-or-hard delete statement
//! with caller's filters AND-joined into the WHERE alongside the
//! delete policy clause, fan-out audit + event, return
//! `BatchSummary`.

use cratestack_core::{AuditOperation, BatchSummary, CoolContext, CoolError, ModelEventKind};

use crate::audit::{build_audit_event, enqueue_audit_event, ensure_audit_table};
use crate::descriptor::{enqueue_event_outbox, ensure_event_outbox_table};
use crate::query::support::{push_action_policy_query, push_filter_query};
use crate::{FilterExpr, ModelDescriptor, sqlx};

pub(super) async fn run_delete_many_in_tx<'tx, M, PK>(
    tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    policy_pool: &sqlx::PgPool,
    descriptor: &'static ModelDescriptor<M, PK>,
    filters: &[FilterExpr],
    ctx: &CoolContext,
) -> Result<(BatchSummary, bool), CoolError>
where
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
{
    if filters.is_empty() {
        return Err(CoolError::Validation(
            "delete_many requires at least one filter — refusing table-wide delete".to_owned(),
        ));
    }

    let emits_event = descriptor.emits(ModelEventKind::Deleted);
    let audit_enabled = descriptor.audit_enabled;
    if emits_event {
        ensure_event_outbox_table(&mut **tx).await?;
    }
    if audit_enabled {
        ensure_audit_table(policy_pool).await?;
    }

    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("");
    let mut wrote_implicit_predicate = false;
    match descriptor.soft_delete_column {
        Some(col) => {
            query.push("UPDATE ").push(descriptor.table_name);
            query.push(" SET ").push(col).push(" = NOW()");
            if let Some(version_col) = descriptor.version_column {
                query
                    .push(", ")
                    .push(version_col)
                    .push(" = ")
                    .push(version_col)
                    .push(" + 1");
            }
            query.push(" WHERE ").push(col).push(" IS NULL");
            wrote_implicit_predicate = true;
        }
        None => {
            query.push("DELETE FROM ").push(descriptor.table_name);
            query.push(" WHERE ");
        }
    }
    if wrote_implicit_predicate {
        query.push(" AND ");
    }
    query.push("(");
    push_filter_query(&mut query, filters);
    query.push(") AND ");
    push_action_policy_query(
        &mut query,
        descriptor.delete_allow_policies,
        descriptor.delete_deny_policies,
        ctx,
    );
    query
        .push(" RETURNING ")
        .push(descriptor.select_projection());

    let removed: Vec<M> = query
        .build_query_as::<M>()
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| CoolError::Database(error.to_string()))?;

    // Fan-out one audit + one outbox entry per actually-deleted row.
    // The RETURNING row IS the audit "before" snapshot for hard
    // deletes; for soft deletes it's the post-tombstone state, but
    // the operation kind still records the delete intent.
    for record in &removed {
        if emits_event {
            enqueue_event_outbox(
                &mut **tx,
                descriptor.schema_name,
                ModelEventKind::Deleted,
                record,
            )
            .await?;
        }
        if audit_enabled {
            let before = serde_json::to_value(record).ok();
            let event = build_audit_event(descriptor, AuditOperation::Delete, before, None, ctx);
            enqueue_audit_event(&mut **tx, &event).await?;
        }
    }

    let total = removed.len();
    Ok((
        BatchSummary {
            total,
            ok: total,
            err: 0,
        },
        emits_event,
    ))
}
