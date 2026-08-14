//! Per-item create: opens a SAVEPOINT, runs validators + create
//! policy + INSERT RETURNING + event/audit fan-out, commits the
//! savepoint on Ok and rolls it back on Err so per-item failures
//! leave no row, no audit row, no outbox entry.

use cratestack_core::{
    AuditEvent, AuditOperation, CratestackContext, CratestackError, ModelEventKind,
};
use sqlx_core::acquire::Acquire as _;

use crate::audit::{build_audit_event, enqueue_audit_event};
use crate::descriptor::enqueue_event_outbox;
use crate::query::support::{
    apply_create_defaults, classify_unique_violation, evaluate_create_policies, find_column_value,
    push_bind_value,
};
use crate::{CreateModelInput, ModelDescriptor, cool_error_from_sqlx, sqlx};

/// Returns `(per_item_result, audit_event)`. `audit_event` is `Some`
/// only when the item succeeded *and* audit is enabled — a savepoint
/// rollback (failed item) never has anything worth fanning out.
/// `audit_event` is not dispatched here: the savepoint this function
/// commits is nested inside `outer`, and `outer` itself isn't
/// committed until every item in the batch has run — a caller batching
/// N items must not fan an item's `AuditSink` event out while N-1 of
/// its siblings could still cause the whole batch to roll back. See
/// `super::create::BatchCreate::run`, which collects these across the
/// loop and dispatches only after `outer` commits.
pub(super) async fn run_create_item<'tx, M, PK, I>(
    outer: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    policy_pool: &sqlx::PgPool,
    descriptor: &'static ModelDescriptor<M, PK>,
    input: I,
    ctx: &CratestackContext,
    emits_event: bool,
    audit_enabled: bool,
) -> Result<(Result<M, CratestackError>, Option<AuditEvent>), CratestackError>
where
    I: CreateModelInput<M>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
{
    let mut item_tx = outer.begin().await.map_err(cool_error_from_sqlx)?;
    let mut audit_event: Option<AuditEvent> = None;

    // All per-item failures funnel through this inner closure so the
    // savepoint commit/rollback decision is centralized below.
    let inner: Result<M, CratestackError> = async {
        input.validate()?;
        let mut values =
            apply_create_defaults(input.sql_values(), descriptor.create_defaults, ctx)?;
        if let Some(version_col) = descriptor.version_column
            && find_column_value(&values, version_col).is_none()
        {
            values.push(crate::SqlColumnValue {
                column: version_col,
                value: crate::SqlValue::Int(0),
            });
        }
        if values.is_empty() {
            return Err(CratestackError::Validation(
                "create input must contain at least one column".to_owned(),
            ));
        }
        if !evaluate_create_policies(
            policy_pool,
            descriptor.create_allow_policies,
            descriptor.create_deny_policies,
            &values,
            ctx,
        )
        .await?
        {
            return Err(CratestackError::Forbidden(
                "create policy denied this operation".to_owned(),
            ));
        }

        let record = insert_one_into_savepoint::<M, PK>(&mut item_tx, descriptor, &values).await?;

        if emits_event {
            enqueue_event_outbox(
                &mut *item_tx,
                descriptor.schema_name,
                ModelEventKind::Created,
                &record,
            )
            .await?;
        }
        if audit_enabled {
            let after = serde_json::to_value(&record).ok();
            let event = build_audit_event(descriptor, AuditOperation::Create, None, after, ctx);
            enqueue_audit_event(&mut *item_tx, &event).await?;
            audit_event = Some(event);
        }
        Ok(record)
    }
    .await;

    match inner {
        Ok(record) => {
            item_tx.commit().await.map_err(cool_error_from_sqlx)?;
            Ok((Ok(record), audit_event))
        }
        Err(item_err) => {
            // ROLLBACK TO SAVEPOINT brings the outer tx back to its
            // pre-savepoint state. If that fails the outer tx is dead
            // and we propagate as the outer Err — no point continuing.
            item_tx.rollback().await.map_err(cool_error_from_sqlx)?;
            Ok((Err(item_err), None))
        }
    }
}

async fn insert_one_into_savepoint<'tx, M, PK>(
    executor: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    descriptor: &'static ModelDescriptor<M, PK>,
    values: &[crate::SqlColumnValue],
) -> Result<M, CratestackError>
where
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
{
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("INSERT INTO ");
    query.push(descriptor.table_name).push(" (");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            query.push(", ");
        }
        query.push(value.column);
    }
    query.push(") VALUES (");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            query.push(", ");
        }
        push_bind_value(&mut query, &value.value);
    }
    query
        .push(") RETURNING ")
        .push(descriptor.select_projection());

    query
        .build_query_as::<M>()
        .fetch_one(&mut **executor)
        .await
        .map_err(classify_unique_violation)
}
