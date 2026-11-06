//! The row a write is about to overwrite or remove, read under a row lock:
//! the audit trail's before-snapshot, and (for an update of an `@@subscribe`
//! model) the pre-image a subscriber's filter-exit event carries
//! (`docs/design/subscriptions-websocket.md` §5.3). Bypasses read policies:
//! it reflects the actual database state, not the caller's filtered view.

use cratestack_core::CratestackError;

use crate::sqlx;
use crate::{ModelDescriptor, SqlColumnValue};

/// `SELECT ... WHERE pk = $1 <lock>`, the lock taking the row so concurrent
/// mutations cannot race us.
async fn fetch_locked<'e, E, M, PK>(
    executor: E,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
    lock: &str,
) -> Result<Option<M>, CratestackError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
    PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
{
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT ");
    query.push(descriptor.select_projection());
    query.push(" FROM ").push(descriptor.table_name);
    query
        .push(" WHERE ")
        .push(descriptor.primary_key)
        .push(" = ");
    query.push_bind(id);
    query.push(lock);
    query
        .build_query_as::<M>()
        .fetch_optional(executor)
        .await
        .map_err(crate::error::cratestack_error_from_sqlx)
}

/// The row a delete is about to remove, under `FOR UPDATE`.
pub(crate) async fn fetch_for_audit<'e, E, M, PK>(
    executor: E,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
) -> Result<Option<M>, CratestackError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
    PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
{
    fetch_locked(executor, descriptor, id, " FOR UPDATE").await
}

/// The row an update that sets `values` is about to overwrite, when something
/// consumes it: the audit trail, or (`keep_before`) a subscriber's
/// filter-exit event.
///
/// The lock is the one the `UPDATE` itself will take, taken up front: `FOR UPDATE`
/// when `values` set the primary key or a unique column (a foreign key may
/// point at the old value), else `FOR NO KEY UPDATE`. Reading under `FOR
/// UPDATE` regardless would make every insert into a referencing table wait on
/// (or deadlock with) every update of a `@@subscribe` model; reading under `FOR
/// NO KEY UPDATE` before a key update would let such an insert take `FOR KEY
/// SHARE` in between, and the `UPDATE`, upgrading, would deadlock against it.
pub(crate) async fn fetch_before_image<'e, E, M, PK>(
    executor: E,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
    keep_before: bool,
    values: &[SqlColumnValue],
) -> Result<Option<M>, CratestackError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
    PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
{
    if !(descriptor.audit_enabled || keep_before) {
        return Ok(None);
    }
    fetch_locked(executor, descriptor, id, update_lock(descriptor, values)).await
}

fn update_lock<M, PK>(
    descriptor: &ModelDescriptor<M, PK>,
    values: &[SqlColumnValue],
) -> &'static str {
    if values
        .iter()
        .any(|value| descriptor.is_key_column(value.column))
    {
        " FOR UPDATE"
    } else {
        " FOR NO KEY UPDATE"
    }
}

#[cfg(test)]
mod tests;
