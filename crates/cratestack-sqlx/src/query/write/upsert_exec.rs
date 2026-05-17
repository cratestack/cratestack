//! Core upsert body: probe the conflict target under a row lock, pick
//! the insert-vs-update branch, run the appropriate policy + audit +
//! outbox writes, then issue the conflict-bearing INSERT.

use cratestack_core::{AuditOperation, CoolContext, CoolError, ModelEventKind};

use crate::audit::{build_audit_event, enqueue_audit_event, ensure_audit_table};
use crate::descriptor::{enqueue_event_outbox, ensure_event_outbox_table};
use crate::query::support::{apply_create_defaults, evaluate_create_policies, find_column_value};
use crate::{ConflictTarget, ModelDescriptor, SqlValue, UpsertModelInput, sqlx};

use super::upsert_sql::{
    row_passes_update_policy, select_for_update_by_conflict_target, upsert_returning_record,
};

/// Returns `(record, emits_any_event)` — the bool lets the caller
/// decide whether to drain the outbox post-commit.
pub(super) async fn run_upsert_in_tx<'tx, M, PK, I>(
    tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    policy_pool: &sqlx::PgPool,
    descriptor: &'static ModelDescriptor<M, PK>,
    input: I,
    conflict_target: ConflictTarget,
    ctx: &CoolContext,
) -> Result<(M, bool), CoolError>
where
    I: UpsertModelInput<M>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
{
    input.validate()?;

    // Compose the full insert value set, including auth-derived
    // defaults and the seeded `@version` column. Mirrors
    // `create_record_with_executor` so insert-branch semantics stay
    // identical to `.create()`.
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

    // Build the conflict-key tuple by looking up each named column's
    // value in the (defaulted) insert set. The PrimaryKey branch
    // keeps the old single-column path so we don't pay an extra
    // lookup on the common case.
    let pk_value = input.primary_key_value();
    let conflict_columns: Vec<(&'static str, SqlValue)> = match conflict_target {
        ConflictTarget::PrimaryKey => vec![(descriptor.primary_key, pk_value)],
        ConflictTarget::Columns(cols) => {
            let mut out = Vec::with_capacity(cols.len());
            for col in cols {
                let value = find_column_value(&insert_values, col).cloned().ok_or_else(|| {
                    CoolError::Validation(format!(
                        "upsert on_conflict references column `{col}` which is not present in the input",
                    ))
                })?;
                out.push((*col, value));
            }
            out
        }
    };

    // Both create and update policies must allow the call. Stricter
    // than "evaluate the path that runs," but pre-flighting a read
    // just to pick the policy slot would leak row existence.
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

    let emits_created = descriptor.emits(ModelEventKind::Created);
    let emits_updated = descriptor.emits(ModelEventKind::Updated);
    let audit_enabled = descriptor.audit_enabled;

    if emits_created || emits_updated {
        ensure_event_outbox_table(&mut **tx).await?;
    }
    if audit_enabled {
        ensure_audit_table(policy_pool).await?;
    }

    // Probe the conflict target under a row-level lock. If a row
    // exists, this is the update branch; otherwise it's the insert
    // branch. The lock serializes concurrent upserts on the same key.
    let before_record =
        select_for_update_by_conflict_target(&mut **tx, descriptor, &conflict_columns).await?;
    let inserted = before_record.is_none();

    if !inserted
        && !row_passes_update_policy(policy_pool, descriptor, &conflict_columns, ctx).await?
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
        upsert_returning_record(&mut **tx, descriptor, &insert_values, conflict_target).await?;

    // Event + audit fan-out, driven off whether the SELECT-FOR-UPDATE
    // saw a row. We don't lean on `xmax = 0`: keeping the
    // discriminator in the runtime (not the SQL) makes the rusqlite
    // mirror trivial.
    let event_kind = if inserted {
        ModelEventKind::Created
    } else {
        ModelEventKind::Updated
    };
    let audit_op = if inserted {
        AuditOperation::Create
    } else {
        AuditOperation::Update
    };
    let emits_event = if inserted {
        emits_created
    } else {
        emits_updated
    };

    if emits_event {
        enqueue_event_outbox(&mut **tx, descriptor.schema_name, event_kind, &record).await?;
    }
    if audit_enabled {
        let after = serde_json::to_value(&record).ok();
        let event = build_audit_event(descriptor, audit_op, before_snapshot, after, ctx);
        enqueue_audit_event(&mut **tx, &event).await?;
    }

    Ok((record, emits_event))
}
