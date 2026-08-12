//! The **routed** write path for SQLite — cratestack#507's "option 3".
//!
//! SQLite-embedded deployments have no `cratestack_event_outbox`
//! equivalent at all (`cratestack-rusqlite`'s own doc comment: "no
//! policies, no audit, no event outbox"), so there is nothing to route
//! for `@@emit` here — [`super::SqliteSource::supports_event_outbox`]
//! stays at the trait default (`false`), and a model with `@@emit` on a
//! SQLite `[target.db]` target is refused by
//! [`crate::api::records::guards::require_write_mode`] unless the target
//! opted into `allow_unsafe_writes`.
//!
//! `@version` bumping *is* routable here — `cratestack-rusqlite`'s own
//! generated code runs the identical `version = version + 1` SQL
//! fragment (see `cratestack-rusqlite/src/batch/update.rs`) — so these
//! two functions apply it for real. Delete needs no override: a hard
//! `DELETE` never touches a version column (nothing survives to bump),
//! so [`super::SqliteSource`]'s `delete_routed` uses the trait default
//! (delegates to `delete`) unchanged.

use std::sync::Arc;

use cratestack_core::Schema;
use rusqlite::Connection;
use tokio::sync::Mutex;

use crate::data::db_errors::map_sqlite_error;
use crate::data::model_info::{resolve_model, version_column};
use crate::data::{DataError, Row};

use super::bindings::build_payload_bindings;
use super::runtime::{fetch_rows, with_conn};
use super::sql::{build_insert_sql, build_update_sql};

pub(super) async fn create_routed(
    schema: &Schema,
    conn: &Arc<Mutex<Connection>>,
    model: &str,
    payload: &Row,
) -> Result<Row, DataError> {
    let (resolved, info) = resolve_model(schema, model)?;
    let resolved = resolved.clone();
    let version_col = version_column(&resolved);
    let (mut cols, mut sql_args) = build_payload_bindings(&info, payload, version_col.as_deref());
    // Seed the optimistic-lock column server-side — mirrors
    // `crate::data::postgres::ops_routed::create_routed`.
    if let Some(v) = &version_col {
        cols.push(v.clone());
        sql_args.push(rusqlite::types::Value::Integer(0));
    }
    let sql = build_insert_sql(&info, &cols);

    let row = with_conn(conn.clone(), move |conn| {
        let params: Vec<&dyn rusqlite::ToSql> =
            sql_args.iter().map(|v| v as &dyn rusqlite::ToSql).collect();
        match fetch_rows(conn, &sql, &params) {
            Ok(rows) => rows
                .into_iter()
                .next()
                .ok_or(DataError::Sqlite(rusqlite::Error::QueryReturnedNoRows)),
            Err(DataError::Sqlite(e)) => {
                Err(map_sqlite_error(Some(&resolved), &e).unwrap_or(DataError::Sqlite(e)))
            }
            Err(other) => Err(other),
        }
    })
    .await?;
    Ok(row)
}

pub(super) async fn update_routed(
    schema: &Schema,
    conn: &Arc<Mutex<Connection>>,
    model: &str,
    pk: &str,
    payload: &Row,
) -> Result<Option<Row>, DataError> {
    let (resolved, info) = resolve_model(schema, model)?;
    let resolved = resolved.clone();
    let version_col = version_column(&resolved);
    let (cols, mut sql_args) = build_payload_bindings(&info, payload, version_col.as_deref());
    sql_args.push(rusqlite::types::Value::Text(pk.to_owned()));
    let sql = build_update_sql(&info, &cols, version_col.as_deref());

    let rows = with_conn(conn.clone(), move |conn| {
        let params: Vec<&dyn rusqlite::ToSql> =
            sql_args.iter().map(|v| v as &dyn rusqlite::ToSql).collect();
        match fetch_rows(conn, &sql, &params) {
            Ok(rows) => Ok(rows),
            Err(DataError::Sqlite(e)) => {
                Err(map_sqlite_error(Some(&resolved), &e).unwrap_or(DataError::Sqlite(e)))
            }
            Err(other) => Err(other),
        }
    })
    .await?;
    Ok(rows.into_iter().next())
}
