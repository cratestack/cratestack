//! SQL emitters used by the upsert per-item driver: pre-FOR-UPDATE
//! probe, update-policy probe, and the final `INSERT ... ON CONFLICT
//! DO UPDATE ... RETURNING` itself.

use cratestack_core::{CoolContext, CoolError};

use crate::query::support::{push_action_policy_query, push_bind_value};
use crate::{ModelDescriptor, SqlValue, sqlx};

pub(super) async fn select_for_update_by_pk_value<'tx, M, PK>(
    executor: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    descriptor: &'static ModelDescriptor<M, PK>,
    pk_value: &SqlValue,
) -> Result<Option<M>, CoolError>
where
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
{
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT ");
    query.push(descriptor.select_projection());
    query.push(" FROM ").push(descriptor.table_name);
    query
        .push(" WHERE ")
        .push(descriptor.primary_key)
        .push(" = ");
    push_bind_value(&mut query, pk_value);
    if let Some(col) = descriptor.soft_delete_column {
        query.push(" AND ").push(col).push(" IS NULL");
    }
    query.push(" FOR UPDATE");

    query
        .build_query_as::<M>()
        .fetch_optional(&mut **executor)
        .await
        .map_err(|error| CoolError::Database(error.to_string()))
}

pub(super) async fn row_passes_update_policy<M, PK>(
    policy_pool: &sqlx::PgPool,
    descriptor: &'static ModelDescriptor<M, PK>,
    pk_value: &SqlValue,
    ctx: &CoolContext,
) -> Result<bool, CoolError> {
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT 1 FROM ");
    query.push(descriptor.table_name);
    query
        .push(" WHERE ")
        .push(descriptor.primary_key)
        .push(" = ");
    push_bind_value(&mut query, pk_value);
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
        .map_err(|error| CoolError::Database(error.to_string()))?;
    Ok(row.is_some())
}

pub(super) async fn upsert_one_in_savepoint<'tx, M, PK>(
    executor: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    descriptor: &'static ModelDescriptor<M, PK>,
    insert_values: &[crate::SqlColumnValue],
) -> Result<M, CoolError>
where
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
    query
        .push(") ON CONFLICT (")
        .push(descriptor.primary_key)
        .push(") DO UPDATE SET ");

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
        .fetch_one(&mut **executor)
        .await
        .map_err(|error| CoolError::Database(error.to_string()))
}
