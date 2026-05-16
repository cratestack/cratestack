//! Per-item upsert: classify insert-vs-update by probing the row
//! under FOR UPDATE, run the create-policy on the prospective values,
//! run the update-policy when an existing row is being modified, emit
//! the right event/audit kind, and persist via
//! `INSERT ... ON CONFLICT DO UPDATE ... RETURNING`.

use cratestack_core::{AuditOperation, CoolContext, CoolError, ModelEventKind};
use sqlx_core::acquire::Acquire as _;

use crate::audit::{build_audit_event, enqueue_audit_event};
use crate::descriptor::enqueue_event_outbox;
use crate::query::support::{
    apply_create_defaults, evaluate_create_policies, find_column_value,
};
use crate::{ModelDescriptor, UpsertModelInput, sqlx};

use super::upsert_sql::{
    row_passes_update_policy, select_for_update_by_pk_value, upsert_one_in_savepoint,
};

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_upsert_item<'tx, M, PK, I>(
    outer: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    policy_pool: &sqlx::PgPool,
    descriptor: &'static ModelDescriptor<M, PK>,
    input: I,
    ctx: &CoolContext,
    emits_created: bool,
    emits_updated: bool,
    audit_enabled: bool,
) -> Result<Result<M, CoolError>, CoolError>
where
    I: UpsertModelInput<M>,
    PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
{
    let mut item_tx = outer
        .begin()
        .await
        .map_err(|error| CoolError::Database(error.to_string()))?;

    let inner: Result<M, CoolError> = async {
        input.validate()?;
        let mut insert_values =
            apply_create_defaults(input.sql_values(), descriptor.create_defaults, ctx)?;
        if let Some(version_col) = descriptor.version_column
            && find_column_value(&insert_values, version_col).is_none()
        {
            insert_values.push(crate::SqlColumnValue {
                column: version_col,
                value: crate::SqlValue::Int(0),
            });
        }
        if insert_values.is_empty() {
            return Err(CoolError::Validation(
                "upsert input must contain at least one column".to_owned(),
            ));
        }
        if !evaluate_create_policies(
            policy_pool,
            descriptor.create_allow_policies,
            descriptor.create_deny_policies,
            &insert_values,
            ctx,
        )
        .await?
        {
            return Err(CoolError::Forbidden(
                "create policy denied this upsert".to_owned(),
            ));
        }

        let pk_value = input.primary_key_value();
        // Probe under FOR UPDATE so the audit before-snapshot is consistent
        // with the row state at the moment of the upsert.
        let before_record =
            select_for_update_by_pk_value(&mut item_tx, descriptor, &pk_value).await?;
        let inserted = before_record.is_none();

        if !inserted
            && !row_passes_update_policy(policy_pool, descriptor, &pk_value, ctx).await?
        {
            return Err(CoolError::Forbidden(
                "update policy denied this upsert".to_owned(),
            ));
        }

        let before_snapshot = if !inserted && audit_enabled {
            before_record
                .as_ref()
                .and_then(|m| serde_json::to_value(m).ok())
        } else {
            None
        };

        let record =
            upsert_one_in_savepoint::<M, PK>(&mut item_tx, descriptor, &insert_values).await?;

        let event_kind = if inserted { ModelEventKind::Created } else { ModelEventKind::Updated };
        let audit_op = if inserted { AuditOperation::Create } else { AuditOperation::Update };
        let emits_this_event = if inserted { emits_created } else { emits_updated };

        if emits_this_event {
            enqueue_event_outbox(&mut *item_tx, descriptor.schema_name, event_kind, &record)
                .await?;
        }
        if audit_enabled {
            let after = serde_json::to_value(&record).ok();
            let event = build_audit_event(descriptor, audit_op, before_snapshot, after, ctx);
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

