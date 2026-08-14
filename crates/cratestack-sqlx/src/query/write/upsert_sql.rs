//! SQL emitters for the upsert path: pre-lock conflict probe, update-
//! policy probe, and the final `INSERT ... ON CONFLICT DO UPDATE
//! ... RETURNING`.

use cratestack_core::{CratestackContext, CratestackError};

use crate::query::support::{classify_unique_violation, push_action_policy_query, push_bind_value};
use crate::{
    ConflictTarget, ModelDescriptor, SqlColumnValue, SqlValue, cratestack_error_from_sqlx, sqlx,
};

/// Probe-with-lock. Bypasses read policies — we need the raw row to
/// drive insert/update branching and to capture the audit
/// before-snapshot. Returns `None` when no row exists (the insert
/// branch).
pub(super) async fn select_for_update_by_conflict_target<'e, E, M, PK>(
    executor: E,
    descriptor: &'static ModelDescriptor<M, PK>,
    conflict: &[(&'static str, SqlValue)],
) -> Result<Option<M>, CratestackError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
{
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT ");
    query.push(descriptor.select_projection());
    query.push(" FROM ").push(descriptor.table_name);
    query.push(" WHERE ");
    for (idx, (column, value)) in conflict.iter().enumerate() {
        if idx > 0 {
            query.push(" AND ");
        }
        query.push(*column).push(" = ");
        push_bind_value(&mut query, value);
    }
    // Soft-deleted rows act as "no row" for upsert purposes: the
    // INSERT branch will then fail on the unique-constraint check,
    // which is the right outcome (refuse to silently revive a
    // tombstone).
    if let Some(col) = descriptor.soft_delete_column {
        query.push(" AND ").push(col).push(" IS NULL");
    }
    query.push(" FOR UPDATE");

    query
        .build_query_as::<M>()
        .fetch_optional(executor)
        .await
        .map_err(cratestack_error_from_sqlx)
}

/// Re-evaluate the update policy against an existing row, using the
/// read pool so the policy predicates can resolve auth/tenancy.
pub(super) async fn row_passes_update_policy<M, PK>(
    policy_pool: &sqlx::PgPool,
    descriptor: &'static ModelDescriptor<M, PK>,
    conflict: &[(&'static str, SqlValue)],
    ctx: &CratestackContext,
) -> Result<bool, CratestackError> {
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT 1 FROM ");
    query.push(descriptor.table_name);
    query.push(" WHERE ");
    for (idx, (column, value)) in conflict.iter().enumerate() {
        if idx > 0 {
            query.push(" AND ");
        }
        query.push(*column).push(" = ");
        push_bind_value(&mut query, value);
    }
    query.push(" AND ");
    push_action_policy_query(
        &mut query,
        descriptor.update_allow_policies,
        descriptor.update_deny_policies,
        ctx,
    );

    let row: Option<(i32,)> = query
        .build_query_as::<(i32,)>()
        .fetch_optional(policy_pool)
        .await
        .map_err(cratestack_error_from_sqlx)?;
    Ok(row.is_some())
}

/// Render and execute the conflict-bearing INSERT. The DO UPDATE
/// clause references only columns in
/// `descriptor.upsert_update_columns` — PK, `@version`, `@readonly`,
/// `@server_only`, and `@default(...)` columns are excluded by
/// construction.
pub(super) async fn upsert_returning_record<'e, E, M, PK>(
    executor: E,
    descriptor: &'static ModelDescriptor<M, PK>,
    insert_values: &[SqlColumnValue],
    conflict_target: ConflictTarget,
) -> Result<M, CratestackError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
{
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("INSERT INTO ");
    query.push(descriptor.table_name).push(" (");
    for (index, value) in insert_values.iter().enumerate() {
        if index > 0 {
            query.push(", ");
        }
        query.push(value.column);
    }
    query.push(") VALUES (");
    for (index, value) in insert_values.iter().enumerate() {
        if index > 0 {
            query.push(", ");
        }
        push_bind_value(&mut query, &value.value);
    }

    query.push(") ON CONFLICT (");
    match conflict_target {
        ConflictTarget::PrimaryKey => {
            query.push(descriptor.primary_key);
        }
        ConflictTarget::Columns(cols) => {
            for (idx, column) in cols.iter().enumerate() {
                if idx > 0 {
                    query.push(", ");
                }
                query.push(*column);
            }
        }
    }
    query.push(") DO UPDATE SET ");

    // If there are no eligible columns to overwrite, fall back to a
    // no-op assignment: touching the PK to itself. This keeps the
    // RETURNING clause working (PG only RETURNs from rows the
    // statement touched).
    //
    // This is deliberately NOT the same mechanism as the per-call-site
    // `.do_nothing()` builder (`upsert_do_nothing_sql::
    // upsert_returning_record_do_nothing`, cratestack#487). They solve
    // different problems and stay separate on purpose:
    //   * This fallback is keyed off `descriptor.upsert_update_columns`
    //     being empty — a property of the *model*, not the call site —
    //     and it is still a `DO UPDATE`. Postgres treats the
    //     self-assignment as a real write: `xmax` bumps, `updated_at`-
    //     style `BEFORE UPDATE` triggers fire, and it generates WAL/
    //     logical-replication traffic, even though no column value
    //     actually changes.
    //   * `.do_nothing()` is an explicit opt-in *per call*, independent
    //     of the model's `upsert_update_columns`, and compiles to a
    //     genuine `ON CONFLICT ... DO NOTHING` — the row is untouched
    //     at the storage layer, no triggers fire, no WAL is generated
    //     for the conflicting row. That distinction is the entire point
    //     of #487: a model with updatable columns had no way to ask for
    //     real non-mutation on a specific call.
    // Merging them would force every empty-`upsert_update_columns`
    // model onto DO-NOTHING semantics (a silent behavior change for
    // existing consumers) or force `.do_nothing()` callers to pay for
    // trigger fan-out they explicitly asked to avoid. Keeping both is a
    // few lines of duplication in exchange for neither call site
    // silently inheriting the other's semantics.
    if descriptor.upsert_update_columns.is_empty() {
        query.push(descriptor.primary_key);
        query.push(" = EXCLUDED.").push(descriptor.primary_key);
    } else {
        for (index, column) in descriptor.upsert_update_columns.iter().enumerate() {
            if index > 0 {
                query.push(", ");
            }
            query.push(*column).push(" = EXCLUDED.").push(*column);
        }
    }
    if let Some(version_col) = descriptor.version_column {
        query
            .push(", ")
            .push(version_col)
            .push(" = ")
            .push(descriptor.table_name)
            .push(".")
            .push(version_col)
            .push(" + 1");
    }

    query
        .push(" RETURNING ")
        .push(descriptor.select_projection());

    query
        .build_query_as::<M>()
        .fetch_one(executor)
        .await
        .map_err(classify_unique_violation)
}
