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

use crate::query::support::{no_row_error, push_action_policy_query};
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
    let probe_id = id.clone();
    match delete_returning_row(executor, descriptor, id, ctx, if_match).await? {
        Some(record) => Ok(record),
        None => Err(no_row_error(policy_pool, descriptor, probe_id, ctx, if_match, "delete").await),
    }
}

/// [`delete_returning_record`] for a statement that runs on `conn`, with
/// the version/policy probe on `conn` too
/// (docs/design/procedure-isolation.md §4.1).
pub(super) async fn delete_record_in_conn<M, PK>(
    conn: &mut sqlx::PgConnection,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
    ctx: &CratestackContext,
    if_match: Option<i64>,
) -> Result<M, CratestackError>
where
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
    PK: Send + Clone + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
{
    let probe_id = id.clone();
    match delete_returning_row(&mut *conn, descriptor, id, ctx, if_match).await? {
        Some(record) => Ok(record),
        None => Err(no_row_error(conn, descriptor, probe_id, ctx, if_match, "delete").await),
    }
}

async fn delete_returning_row<'e, E, M, PK>(
    executor: E,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
    ctx: &CratestackContext,
    if_match: Option<i64>,
) -> Result<Option<M>, CratestackError>
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

    query
        .build_query_as::<M>()
        .fetch_optional(executor)
        .await
        .map_err(cratestack_error_from_sqlx)
}
