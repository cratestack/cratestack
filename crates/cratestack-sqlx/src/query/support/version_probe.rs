//! Shared version-probe for optimistic-lock mutation paths.
//!
//! Both the versioned `UPDATE` (`query/write/update_exec.rs`) and the
//! versioned `DELETE` (`query/write/delete_exec.rs`) add `AND version =
//! $expected` to their `WHERE` clause, so a stale `If-Match` and a
//! genuine policy denial are indistinguishable from "no row returned"
//! alone. [`probe_current_version`] re-reads the row through the
//! *read* policy (not the mutation's own allow/deny set) to tell them
//! apart: if the caller can see the row and its version differs from
//! what was expected, the mutation reports `412` instead of folding
//! that case into the generic `403`.

use cratestack_core::{CratestackContext, CratestackError};

use super::push_action_policy_query;
use crate::{ModelDescriptor, cratestack_error_from_sqlx, sqlx};

/// Read the current version of a row using the read policy. Returns
/// `None` if the caller cannot see the row (so the outer code
/// preserves the existing Forbidden-on-no-row semantics).
pub(crate) async fn probe_current_version<M, PK>(
    policy_pool: &sqlx::PgPool,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
    version_col: &'static str,
    ctx: &CratestackContext,
) -> Result<Option<i64>, CratestackError>
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
        .map_err(cratestack_error_from_sqlx)?;
    Ok(row.map(|(v,)| v))
}
