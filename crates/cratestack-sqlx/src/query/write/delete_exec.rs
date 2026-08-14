//! Generic-over-Executor delete helper used by both the pool and
//! transaction paths in [`super::delete`]. Soft-delete and hard-delete
//! both end in `RETURNING projection`, but that row is the pre-delete
//! state only for a hard delete — a soft delete is an `UPDATE`, so its
//! `RETURNING` row is post-tombstone. `super::delete` accounts for
//! that when it builds the audit snapshot.
//!
//! `if_match` gates on `descriptor.version_column` alone, the same way
//! the update path does — it's independent of `soft_delete_column`, so
//! a `@version` model gets `If-Match` enforcement whether or not it is
//! also `@@soft_delete`, and a plain hard-delete `@version` model is
//! covered identically. See `query/write/update_exec.rs` for the
//! sibling implementation this mirrors.

use cratestack_core::{CratestackContext, CratestackError};

use crate::query::support::{probe_current_version, push_action_policy_query};
use crate::{ModelDescriptor, cratestack_error_from_sqlx, sqlx};

pub(super) async fn delete_returning_record<'e, E, M, PK>(
    executor: E,
    policy_pool: &sqlx::PgPool,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
    ctx: &CratestackContext,
    if_match: Option<i64>,
) -> Result<M, CratestackError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
    PK: Send + Clone + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
{
    let version_column = descriptor.version_column;
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("");
    match descriptor.soft_delete_column {
        Some(col) => {
            // Soft-delete: tombstone the row and bump version (if any)
            // so optimistic-lock semantics on subsequent updates stay
            // coherent.
            query.push("UPDATE ").push(descriptor.table_name);
            query.push(" SET ").push(col).push(" = NOW()");
            if let Some(version_col) = version_column {
                query
                    .push(", ")
                    .push(version_col)
                    .push(" = ")
                    .push(version_col)
                    .push(" + 1");
            }
            query.push(" WHERE ");
            query.push(col).push(" IS NULL AND ");
            query.push(descriptor.primary_key).push(" = ");
        }
        None => {
            query.push("DELETE FROM ").push(descriptor.table_name);
            query.push(" WHERE ");
            query.push(descriptor.primary_key).push(" = ");
        }
    }
    let id_for_probe = id.clone();
    query.push_bind(id);
    if let (Some(version_col), Some(expected)) = (version_column, if_match) {
        query.push(" AND ").push(version_col).push(" = ");
        query.push_bind(expected);
    }
    query.push(" AND ");
    push_action_policy_query(
        &mut query,
        descriptor.delete_allow_policies,
        descriptor.delete_deny_policies,
        ctx,
    );
    query
        .push(" RETURNING ")
        .push(descriptor.select_projection());

    let outcome = query
        .build_query_as::<M>()
        .fetch_optional(executor)
        .await
        .map_err(cratestack_error_from_sqlx)?;
    match outcome {
        Some(record) => Ok(record),
        None => {
            // Same disambiguation as the update path: a versioned
            // delete that matched no row might be a stale `If-Match`
            // rather than a policy denial, so re-probe under the read
            // policy before falling back to the generic Forbidden.
            if let (Some(version_col), Some(expected)) = (version_column, if_match)
                && let Some(current) =
                    probe_current_version(policy_pool, descriptor, id_for_probe, version_col, ctx)
                        .await?
                && current != expected
            {
                return Err(CratestackError::PreconditionFailed(format!(
                    "version mismatch: expected {expected}, found {current}",
                )));
            }
            Err(CratestackError::Forbidden(
                "delete policy denied this operation".to_owned(),
            ))
        }
    }
}
