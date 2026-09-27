//! Generic-over-Executor update helpers used by single-row UPDATE
//! paths. Builds `UPDATE ... SET ... WHERE pk = $X [AND version = $Y]
//! AND policy(...) RETURNING ...`, with version-mismatch detection via
//! a read-policy probe.
//!
//! Two entry points, differing only in where that probe runs:
//! [`update_record_with_executor`] (public; the probe runs on the pool it
//! is handed) and [`update_record_in_conn`] (the probe runs on the
//! statement's own connection inside an `@isolation` procedure, on the
//! pool otherwise; see docs/design/procedure-isolation.md §4.1).

use cratestack_core::{CratestackContext, CratestackError};

use crate::query::support::{
    PolicyDb, classify_unique_violation, no_row_error, push_action_policy_query, push_bind_value,
};
use crate::{ModelDescriptor, SqlColumnValue, SqlxRuntime, UpdateModelInput, sqlx};

pub async fn update_record_with_executor<'e, E, M, PK, I>(
    executor: E,
    policy_pool: &sqlx::PgPool,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
    input: I,
    ctx: &CratestackContext,
    if_match: Option<i64>,
) -> Result<M, CratestackError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    I: UpdateModelInput<M>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    PK: Send + Clone + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
{
    let values = update_values(input)?;
    let probe_id = id.clone();
    match update_returning_record(executor, descriptor, id, &values, ctx, if_match).await? {
        Some(record) => Ok(record),
        None => Err(no_row_error(policy_pool, descriptor, probe_id, ctx, if_match, "update").await),
    }
}

/// [`update_record_with_executor`] for a write that runs on `conn`, with
/// the version/policy probe wherever [`PolicyDb::of`] puts it.
pub(crate) async fn update_record_in_conn<M, PK, I>(
    runtime: &SqlxRuntime,
    conn: &mut sqlx::PgConnection,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
    input: I,
    ctx: &CratestackContext,
    if_match: Option<i64>,
) -> Result<M, CratestackError>
where
    I: UpdateModelInput<M>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    PK: Send + Clone + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
{
    let values = update_values(input)?;
    let probe_id = id.clone();
    match update_returning_record(&mut *conn, descriptor, id, &values, ctx, if_match).await? {
        Some(record) => Ok(record),
        None => Err(match PolicyDb::of(runtime, conn) {
            PolicyDb::Pool(pool) => {
                no_row_error(pool, descriptor, probe_id, ctx, if_match, "update").await
            }
            PolicyDb::Conn(conn) => {
                no_row_error(conn, descriptor, probe_id, ctx, if_match, "update").await
            }
        }),
    }
}

fn update_values<M, I: UpdateModelInput<M>>(
    input: I,
) -> Result<Vec<SqlColumnValue>, CratestackError> {
    input.validate()?;
    let values = input.sql_values();
    if values.is_empty() {
        return Err(CratestackError::Validation(
            "update input must contain at least one changed column".to_owned(),
        ));
    }
    Ok(values)
}

async fn update_returning_record<'e, E, M, PK>(
    executor: E,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
    values: &[crate::SqlColumnValue],
    ctx: &CratestackContext,
    if_match: Option<i64>,
) -> Result<Option<M>, CratestackError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
    PK: Send + Clone + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
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

    query
        .build_query_as::<M>()
        .fetch_optional(executor)
        .await
        .map_err(classify_unique_violation)
}
