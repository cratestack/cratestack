//! WHERE-clause assembly + per-action authorization probes.
//! [`push_scoped_conditions`] is the single entry point used by every
//! read-path delegate: soft-delete + caller filters + optional PK + the
//! action policy slot. [`authorize_record_action`] runs a one-shot
//! `SELECT 1 WHERE policy(...)` for mutation preflight.

use cratestack_core::{CoolContext, CoolError};

use cratestack_sql::ReadSource;

use crate::{FilterExpr, ReadPolicy, SqlxRuntime, sqlx};

use super::filter::push_filter_query;
use super::policy::push_action_policy_query;

/// Which policy slot to consult when filtering rows from a read query.
/// Schemas can declare separate `@@allow("list", ...)` (folded into
/// `read_*`) and `@@allow("detail", ...)` (folded into `detail_*`)
/// predicates; the right slot depends on what kind of read is happening.
/// Bulk and listing operations apply List; single-row lookups (where the
/// caller is asking for a specific row by PK or unique key) apply
/// Detail. The toggle is exposed on `FindUnique` via `.as_detail()` /
/// `.as_list()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadPolicyKind {
    /// `read_*` policies — used by list-style reads (`find_many`,
    /// `batch_get`, scoped updates/deletes that filter by PK).
    List,
    /// `detail_*` policies — used by single-row lookups
    /// (`find_unique`). Falls back to the list policies when the schema
    /// hasn't declared explicit detail rules.
    Detail,
}

pub(crate) fn push_scoped_conditions<'a, M, PK, Id>(
    query: &mut sqlx::QueryBuilder<'a, sqlx::Postgres>,
    descriptor: &dyn ReadSource<M, PK>,
    filters: &[FilterExpr],
    primary_key: Option<(&'static str, Id)>,
    ctx: &CoolContext,
    policy_kind: ReadPolicyKind,
) where
    Id: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres> + 'a,
{
    query.push(" WHERE ");

    let mut wrote_clause = false;
    // Soft-delete filter: hide tombstoned rows from every read. Banks
    // treat the audit log as source of truth for what changed; this
    // just prevents deleted rows from leaking back into responses.
    // Views always report `None` here — the view's SQL body is
    // responsible for filtering soft-deleted source rows.
    if let Some(col) = descriptor.soft_delete_column() {
        query.push(col).push(" IS NULL");
        wrote_clause = true;
    }
    if !filters.is_empty() {
        if wrote_clause {
            query.push(" AND ");
        }
        push_filter_query(query, filters);
        wrote_clause = true;
    }

    if let Some((primary_key, id)) = primary_key {
        if wrote_clause {
            query.push(" AND ");
        }
        query.push(primary_key).push(" = ");
        query.push_bind(id);
        wrote_clause = true;
    }

    if wrote_clause {
        query.push(" AND ");
    }
    let (allow, deny) = match policy_kind {
        ReadPolicyKind::List => (
            descriptor.read_allow_policies(),
            descriptor.read_deny_policies(),
        ),
        ReadPolicyKind::Detail => (
            descriptor.detail_allow_policies(),
            descriptor.detail_deny_policies(),
        ),
    };
    push_action_policy_query(query, allow, deny, ctx);
}

pub(crate) async fn authorize_record_action<M, PK>(
    runtime: &SqlxRuntime,
    descriptor: &'static dyn ReadSource<M, PK>,
    id: PK,
    allow_policies: &[ReadPolicy],
    deny_policies: &[ReadPolicy],
    ctx: &CoolContext,
    action_name: &str,
) -> Result<(), CoolError>
where
    PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
{
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT 1 FROM ");
    query
        .push(descriptor.table_name())
        .push(" WHERE ")
        .push(descriptor.primary_key())
        .push(" = ");
    query.push_bind(id);
    query.push(" AND ");
    push_action_policy_query(&mut query, allow_policies, deny_policies, ctx);
    query.push(" LIMIT 1");

    let authorized = query
        .build_query_scalar::<i32>()
        .fetch_optional(runtime.pool())
        .await
        .map_err(|error| CoolError::Database(error.to_string()))?
        .is_some();

    if authorized {
        Ok(())
    } else {
        Err(CoolError::Forbidden(format!(
            "{action_name} policy denied this operation"
        )))
    }
}
