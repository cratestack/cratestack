//! Per-item update: SAVEPOINT, optional FOR UPDATE before-snapshot
//! probe, UPDATE ... RETURNING with policy + If-Match, event/audit
//! fan-out. Per-item failures rollback the savepoint.

use cratestack_core::{AuditOperation, CoolContext, CoolError, ModelEventKind};
use sqlx_core::acquire::Acquire as _;

use crate::audit::{build_audit_event, enqueue_audit_event, fetch_for_audit};
use crate::descriptor::enqueue_event_outbox;
use crate::query::support::{push_action_policy_query, push_bind_value};
use crate::{ModelDescriptor, UpdateModelInput, sqlx};

pub(super) async fn run_update_item<'tx, M, PK, I>(
    outer: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
    input: I,
    if_match: Option<i64>,
    ctx: &CoolContext,
    emits_event: bool,
    audit_enabled: bool,
) -> Result<Result<M, CoolError>, CoolError>
where
    I: UpdateModelInput<M>,
    PK: Clone + Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
{
    let mut item_tx = outer
        .begin()
        .await
        .map_err(|error| CoolError::Database(error.to_string()))?;

    let inner: Result<M, CoolError> = async {
        if descriptor.version_column.is_some() && if_match.is_none() {
            return Err(CoolError::PreconditionFailed(
                "If-Match required for versioned model".to_owned(),
            ));
        }
        input.validate()?;
        let values = input.sql_values();
        if values.is_empty() {
            return Err(CoolError::Validation(
                "update input must contain at least one changed column".to_owned(),
            ));
        }

        // Capture before-snapshot under FOR UPDATE for clean audit timing.
        let before = if audit_enabled {
            fetch_for_audit(&mut *item_tx, descriptor, id.clone()).await?
        } else {
            None
        };

        let record =
            update_one_in_savepoint(&mut item_tx, descriptor, id, &values, ctx, if_match).await?;

        if emits_event {
            enqueue_event_outbox(
                &mut *item_tx,
                descriptor.schema_name,
                ModelEventKind::Updated,
                &record,
            )
            .await?;
        }
        if audit_enabled {
            let before_snapshot = before.as_ref().and_then(|m| serde_json::to_value(m).ok());
            let after = serde_json::to_value(&record).ok();
            let event = build_audit_event(
                descriptor,
                AuditOperation::Update,
                before_snapshot,
                after,
                ctx,
            );
            enqueue_audit_event(&mut *item_tx, &event).await?;
        }
        Ok(record)
    }
    .await;

    match inner {
        Ok(record) => {
            item_tx
                .commit()
                .await
                .map_err(|error| CoolError::Database(error.to_string()))?;
            Ok(Ok(record))
        }
        Err(item_err) => {
            item_tx
                .rollback()
                .await
                .map_err(|error| CoolError::Database(error.to_string()))?;
            Ok(Err(item_err))
        }
    }
}

async fn update_one_in_savepoint<'tx, M, PK>(
    executor: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
    values: &[crate::SqlColumnValue],
    ctx: &CoolContext,
    if_match: Option<i64>,
) -> Result<M, CoolError>
where
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
    PK: Clone + Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
{
    let version_column = descriptor.version_column;
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("UPDATE ");
    query.push(descriptor.table_name).push(" SET ");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
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
    query
        .push(" WHERE ")
        .push(descriptor.primary_key)
        .push(" = ");
    query.push_bind(id);
    if let (Some(version_col), Some(expected)) = (version_column, if_match) {
        query.push(" AND ").push(version_col).push(" = ");
        query.push_bind(expected);
    }
    query.push(" AND ");
    push_action_policy_query(
        &mut query,
        descriptor.update_allow_policies,
        descriptor.update_deny_policies,
        ctx,
    );
    query
        .push(" RETURNING ")
        .push(descriptor.select_projection());

    let outcome = query
        .build_query_as::<M>()
        .fetch_optional(&mut **executor)
        .await
        .map_err(|error| CoolError::Database(error.to_string()))?;
    match outcome {
        Some(record) => Ok(record),
        None => {
            // Could be: row missing, policy denied, version mismatch,
            // soft-deleted. Probing to discriminate adds round-trips;
            // we report Forbidden when there's no if_match,
            // PreconditionFailed when there is. Either way the caller's
            // recovery is the same: refetch & retry.
            if if_match.is_some() {
                Err(CoolError::PreconditionFailed(
                    "version mismatch or row missing".to_owned(),
                ))
            } else {
                Err(CoolError::Forbidden(
                    "update policy denied or row missing".to_owned(),
                ))
            }
        }
    }
}
