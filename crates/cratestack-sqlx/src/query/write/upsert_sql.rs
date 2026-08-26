//! SQL emitters for the upsert path: the pre-lock conflict probe and
//! the update-policy probe. The DO UPDATE INSERT emitter itself lives
//! in `upsert_do_update_sql.rs` — split out purely to stay under this
//! codebase's ~200-LoC-per-file convention, not a behavioral boundary.

use cratestack_core::{CratestackContext, CratestackError};

use crate::query::support::{push_action_policy_query, push_bind_value};
use crate::{ModelDescriptor, SqlValue, cratestack_error_from_sqlx, sqlx};

/// Probe-with-lock. Bypasses read policies — we need the raw row to
/// drive insert/update branching and to capture the audit
/// before-snapshot. Returns `None` when no row exists (the insert
/// branch).
///
/// `predicate` MUST be the same [`ConflictTarget::predicate`]
/// (cratestack#741) the caller's `ON CONFLICT` targets. Against a
/// partial unique index, filtering on the conflict columns alone can
/// match a row the index does not cover — this probe is the "outcome"
/// half of that correctness requirement; `upsert_do_nothing_sql`/
/// `upsert_sql`'s emitted `ON CONFLICT ... WHERE ...` is the "SQL
/// shape" half. Both must agree or the caller is handed a verdict
/// (`Inserted`/`Existing`) that doesn't match what the database did.
pub(super) async fn select_for_update_by_conflict_target<'e, E, M, PK>(
    executor: E,
    descriptor: &'static ModelDescriptor<M, PK>,
    conflict: &[(&'static str, SqlValue)],
    predicate: Option<&'static str>,
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
    if let Some(predicate) = predicate {
        query.push(" AND (").push(predicate).push(")");
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
///
/// `predicate` carries the same partial-index predicate as
/// [`select_for_update_by_conflict_target`] and for the same reason:
/// without it, the conflict columns alone could match a row outside a
/// partial index's uniqueness domain, letting the wrong row's policy
/// gate this call.
pub(super) async fn row_passes_update_policy<M, PK>(
    policy_pool: &sqlx::PgPool,
    descriptor: &'static ModelDescriptor<M, PK>,
    conflict: &[(&'static str, SqlValue)],
    predicate: Option<&'static str>,
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
    if let Some(predicate) = predicate {
        query.push(" AND (").push(predicate).push(")");
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
