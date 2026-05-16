//! Generic-over-Executor create helper used by both the pool and
//! transaction paths in [`super::create`]. Validates, applies
//! auth-defaults, seeds `@version`, evaluates create policies, then
//! runs `INSERT ... RETURNING`.

use cratestack_core::{CoolContext, CoolError};

use crate::query::support::{
    apply_create_defaults, evaluate_create_policies, find_column_value, push_bind_value,
};
use crate::{CreateModelInput, ModelDescriptor, sqlx};

pub async fn create_record_with_executor<'e, E, M, PK, I>(
    executor: E,
    policy_pool: &sqlx::PgPool,
    descriptor: &'static ModelDescriptor<M, PK>,
    input: I,
    ctx: &CoolContext,
) -> Result<M, CoolError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    I: CreateModelInput<M>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
{
    input.validate()?;
    let mut values = apply_create_defaults(input.sql_values(), descriptor.create_defaults, ctx)?;
    // Seed the optimistic-lock column server-side. `@version` is
    // excluded from the generated Create input so clients can't pick
    // the initial value, and the column has no SQL `DEFAULT`. Done
    // after `apply_create_defaults` so `@default`-driven overrides
    // still win if a schema ever lands one.
    if let Some(version_col) = descriptor.version_column
        && find_column_value(&values, version_col).is_none()
    {
        values.push(crate::SqlColumnValue {
            column: version_col,
            value: crate::SqlValue::Int(0),
        });
    }
    if values.is_empty() {
        return Err(CoolError::Validation(
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
        return Err(CoolError::Forbidden(
            "create policy denied this operation".to_owned(),
        ));
    }

    insert_returning_record(executor, descriptor, &values).await
}

async fn insert_returning_record<'e, E, M, PK>(
    executor: E,
    descriptor: &'static ModelDescriptor<M, PK>,
    values: &[crate::SqlColumnValue],
) -> Result<M, CoolError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
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
        .fetch_one(executor)
        .await
        .map_err(|error| CoolError::Database(error.to_string()))
}
