//! Generic-over-executor query runners shared by the unrouted
//! ([`super::ops`]) and routed ([`super::ops_routed`]) write paths.
//!
//! The SQL differs between the two (whether `@version`/event-outbox
//! semantics are applied — see `super::ops_routed`'s module doc for
//! cratestack#507's "option 3"), but "run this `RETURNING`-shaped query
//! against either the pool directly or an open transaction, decode the
//! JSON row, map constraint failures" is identical in both, so it lives
//! once here rather than twice.

use cratestack_core::Model;
// `AssertSqlSafe` (sqlx 0.9, #3723): every `sql` reaching these runners is
// assembled by `super::sql`'s builders from schema-derived identifiers only —
// all user/row data arrives through `bind_typed`/`.bind()` as `$n` parameters,
// never interpolated. The builders are the audited boundary; asserting here
// rather than at each of the ~8 call sites keeps that assertion in one place.
use sqlx_core::row::Row as _;
use sqlx_core::sql_str::AssertSqlSafe;
use sqlx_postgres::{PgRow, Postgres};

use crate::data::db_errors::map_pg_error;
use crate::data::{DataError, Row};

use super::bindings::{TypedValue, bind_typed};

pub(super) async fn insert_returning<'e, E>(
    executor: E,
    sql: &str,
    binds: &[TypedValue],
    resolved: &Model,
) -> Result<Row, DataError>
where
    E: sqlx_core::executor::Executor<'e, Database = Postgres>,
{
    let mut q = sqlx_core::query::query(AssertSqlSafe(sql));
    for value in binds {
        q = bind_typed(q, value);
    }
    let row = match q.fetch_one(executor).await {
        Ok(r) => r,
        Err(e) => return Err(map_pg_error(Some(resolved), &e).unwrap_or(DataError::Db(e))),
    };
    decode_row(row)
}

pub(super) async fn update_returning<'e, E>(
    executor: E,
    sql: &str,
    binds: &[TypedValue],
    pk: &str,
    resolved: &Model,
) -> Result<Option<Row>, DataError>
where
    E: sqlx_core::executor::Executor<'e, Database = Postgres>,
{
    let mut q = sqlx_core::query::query(AssertSqlSafe(sql));
    for value in binds {
        q = bind_typed(q, value);
    }
    q = q.bind(pk);
    let row = match q.fetch_optional(executor).await {
        Ok(r) => r,
        Err(e) => return Err(map_pg_error(Some(resolved), &e).unwrap_or(DataError::Db(e))),
    };
    decode_optional(row)
}

pub(super) async fn delete_returning<'e, E>(
    executor: E,
    sql: &str,
    pk: &str,
    resolved: &Model,
) -> Result<Option<Row>, DataError>
where
    E: sqlx_core::executor::Executor<'e, Database = Postgres>,
{
    let row = match sqlx_core::query::query(AssertSqlSafe(sql))
        .bind(pk)
        .fetch_optional(executor)
        .await
    {
        Ok(r) => r,
        Err(e) => return Err(map_pg_error(Some(resolved), &e).unwrap_or(DataError::Db(e))),
    };
    decode_optional(row)
}

pub(super) fn decode_rows(rows: Vec<PgRow>) -> Result<Vec<Row>, DataError> {
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let value: serde_json::Value = row.try_get(0)?;
        if let serde_json::Value::Object(map) = value {
            out.push(map);
        }
    }
    Ok(out)
}

fn decode_row(row: PgRow) -> Result<Row, DataError> {
    let value: serde_json::Value = row.try_get(0)?;
    match value {
        serde_json::Value::Object(map) => Ok(map),
        _ => Err(DataError::Unsupported {
            what: "INSERT … RETURNING did not produce a JSON object",
        }),
    }
}

pub(super) fn decode_optional(row: Option<PgRow>) -> Result<Option<Row>, DataError> {
    match row {
        None => Ok(None),
        Some(r) => {
            let value: serde_json::Value = r.try_get(0)?;
            Ok(match value {
                serde_json::Value::Object(map) => Some(map),
                _ => None,
            })
        }
    }
}
