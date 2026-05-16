//! Generic-over-Executor update helpers used by single-row UPDATE
//! paths. Builds `UPDATE ... SET ... WHERE pk = $X [AND version = $Y]
//! AND policy(...) RETURNING ...`, with version-mismatch detection via
//! a read-policy probe.

use cratestack_core::{CoolContext, CoolError};

use crate::query::support::{push_action_policy_query, push_bind_value};
use crate::{ModelDescriptor, UpdateModelInput, sqlx};

pub async fn update_record_with_executor<'e, E, M, PK, I>(
    executor: E,
    policy_pool: &sqlx::PgPool,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
    input: I,
    ctx: &CoolContext,
    if_match: Option<i64>,
) -> Result<M, CoolError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    I: UpdateModelInput<M>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    PK: Send + Clone + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
{
    input.validate()?;
    let values = input.sql_values();
    if values.is_empty() {
        return Err(CoolError::Validation(
            "update input must contain at least one changed column".to_owned(),
        ));
    }

    update_returning_record(executor, policy_pool, descriptor, id, &values, ctx, if_match).await
}

#[allow(clippy::too_many_arguments)]
async fn update_returning_record<'e, E, M, PK>(
    executor: E,
    policy_pool: &sqlx::PgPool,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
    values: &[crate::SqlColumnValue],
    ctx: &CoolContext,
    if_match: Option<i64>,
) -> Result<M, CoolError>
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
    let id_for_probe = id.clone();
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

    let outcome = query
        .build_query_as::<M>()
        .fetch_optional(executor)
        .await
        .map_err(|error| CoolError::Database(error.to_string()))?;
    match outcome {
        Some(record) => Ok(record),
        None => {
            // If this is a versioned update, distinguish "stale
            // version" from a true policy denial via the read-policy
            // probe. If the caller can't see the row, we keep
            // returning Forbidden so policy denials remain
            // indistinguishable from missing rows.
            if let (Some(version_col), Some(expected)) = (version_column, if_match)
                && let Some(current) =
                    probe_current_version(policy_pool, descriptor, id_for_probe, version_col, ctx)
                        .await?
                && current != expected
            {
                return Err(CoolError::PreconditionFailed(format!(
                    "version mismatch: expected {expected}, found {current}",
                )));
            }
            Err(CoolError::Forbidden(
                "update policy denied this operation".to_owned(),
            ))
        }
    }
}

/// Read the current version of a row using the read policy. Returns
/// `None` if the caller cannot see the row (so the outer code
/// preserves the existing Forbidden-on-no-row semantics).
async fn probe_current_version<M, PK>(
    policy_pool: &sqlx::PgPool,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
    version_col: &'static str,
    ctx: &CoolContext,
) -> Result<Option<i64>, CoolError>
where
    PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
{
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT ");
    query.push(version_col);
    query.push(" FROM ").push(descriptor.table_name);
    query
        .push(" WHERE ")
        .push(descriptor.primary_key)
        .push(" = ");
    query.push_bind(id);
    query.push(" AND ");
    push_action_policy_query(
        &mut query,
        descriptor.read_allow_policies,
        descriptor.read_deny_policies,
        ctx,
    );

    let row: Option<(i64,)> = query
        .build_query_as::<(i64,)>()
        .fetch_optional(policy_pool)
        .await
        .map_err(|error| CoolError::Database(error.to_string()))?;
    Ok(row.map(|(v,)| v))
}
