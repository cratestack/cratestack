//! Generic-over-Executor delete helper used by both the pool and
//! transaction paths in [`super::delete`]. Soft-delete and hard-delete
//! both end in `RETURNING projection` so the audit "before" snapshot
//! is the row's pre-delete state.

use cratestack_core::{CoolContext, CoolError};

use crate::query::support::push_action_policy_query;
use crate::{ModelDescriptor, sqlx};

pub(super) async fn delete_returning_record<'e, E, M, PK>(
    executor: E,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
    ctx: &CoolContext,
) -> Result<M, CoolError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
    PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
{
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("");
    match descriptor.soft_delete_column {
        Some(col) => {
            // Soft-delete: tombstone the row and bump version (if any)
            // so optimistic-lock semantics on subsequent updates stay
            // coherent.
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
            query.push(" WHERE ");
            query.push(col).push(" IS NULL AND ");
            query.push(descriptor.primary_key).push(" = ");
            query.push_bind(id);
        }
        None => {
            query.push("DELETE FROM ").push(descriptor.table_name);
            query.push(" WHERE ");
            query.push(descriptor.primary_key).push(" = ");
            query.push_bind(id);
        }
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
        .map_err(|error| CoolError::Database(error.to_string()))?
        .ok_or_else(|| CoolError::Forbidden("delete policy denied this operation".to_owned()))
}
