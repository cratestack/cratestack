//! Execution-side operations for the SQLite source.
//!
//! Each function composes a SQL builder ([`super::sql`]), payload
//! bindings ([`super::bindings`]), and the blocking-pool runtime
//! ([`super::runtime`]) into one CRUD action. Constraint failures are
//! mapped to [`DataError::Validation`] via [`crate::data::db_errors`]
//! so the UI can show per-field errors.

use std::sync::Arc;

use cratestack_core::Schema;
use rusqlite::Connection;
use tokio::sync::Mutex;

use crate::data::common::{clamp_limit, next_cursor};
use crate::data::db_errors::map_sqlite_error;
use crate::data::model_info::{PkCast, find_pk_field, resolve_model};
use crate::data::{DataError, Page, PageRequest, Row};

use super::bindings::build_payload_bindings;
use super::runtime::{fetch_rows, with_conn};
use super::sql::{
    build_delete_sql, build_get_sql, build_insert_sql, build_list_on_column_sql, build_list_sql,
    build_update_sql,
};

pub(super) async fn list(
    schema: &Schema,
    conn: &Arc<Mutex<Connection>>,
    model: &str,
    page: PageRequest<'_>,
) -> Result<Page, DataError> {
    let (resolved_model, info) = resolve_model(schema, model)?;
    let limit = clamp_limit(page.limit);
    let sql = build_list_sql(&info, limit);
    let pk_field_name = find_pk_field(resolved_model)
        .map(|f| f.name.clone())
        .expect("resolve_model returns an error when there is no @id");
    let cursor_owned = page.cursor.map(str::to_owned);

    let rows = with_conn(conn.clone(), move |conn| match cursor_owned {
        Some(s) => fetch_rows(conn, &sql, &[&s]),
        None => fetch_rows(conn, &sql, &[&rusqlite::types::Null]),
    })
    .await?;

    let next_cursor = next_cursor(&rows, &pk_field_name, limit);
    Ok(Page { rows, next_cursor })
}

pub(super) async fn get(
    schema: &Schema,
    conn: &Arc<Mutex<Connection>>,
    model: &str,
    pk: &str,
) -> Result<Option<Row>, DataError> {
    let (_, info) = resolve_model(schema, model)?;
    let sql = build_get_sql(&info);
    let pk_owned = pk.to_owned();

    let rows = with_conn(conn.clone(), move |conn| {
        fetch_rows(conn, &sql, &[&pk_owned])
    })
    .await?;

    Ok(rows.into_iter().next())
}

pub(super) async fn create(
    schema: &Schema,
    conn: &Arc<Mutex<Connection>>,
    model: &str,
    payload: &Row,
) -> Result<Row, DataError> {
    let (resolved, info) = resolve_model(schema, model)?;
    let resolved = resolved.clone();
    let (cols, sql_args) = build_payload_bindings(&info, payload);
    let sql = build_insert_sql(&info, &cols);

    let row = with_conn(conn.clone(), move |conn| {
        let params: Vec<&dyn rusqlite::ToSql> =
            sql_args.iter().map(|v| v as &dyn rusqlite::ToSql).collect();
        match fetch_rows(conn, &sql, &params) {
            Ok(rows) => rows
                .into_iter()
                .next()
                .ok_or(DataError::Sqlite(rusqlite::Error::QueryReturnedNoRows)),
            Err(DataError::Sqlite(e)) => Err(map_sqlite_error(Some(&resolved), &e)
                .unwrap_or(DataError::Sqlite(e))),
            Err(other) => Err(other),
        }
    })
    .await?;
    Ok(row)
}

pub(super) async fn update(
    schema: &Schema,
    conn: &Arc<Mutex<Connection>>,
    model: &str,
    pk: &str,
    payload: &Row,
) -> Result<Option<Row>, DataError> {
    let (resolved, info) = resolve_model(schema, model)?;
    let resolved = resolved.clone();
    let (cols, mut sql_args) = build_payload_bindings(&info, payload);
    sql_args.push(rusqlite::types::Value::Text(pk.to_owned()));
    let sql = build_update_sql(&info, &cols);

    let rows = with_conn(conn.clone(), move |conn| {
        let params: Vec<&dyn rusqlite::ToSql> =
            sql_args.iter().map(|v| v as &dyn rusqlite::ToSql).collect();
        match fetch_rows(conn, &sql, &params) {
            Ok(rows) => Ok(rows),
            Err(DataError::Sqlite(e)) => Err(map_sqlite_error(Some(&resolved), &e)
                .unwrap_or(DataError::Sqlite(e))),
            Err(other) => Err(other),
        }
    })
    .await?;
    Ok(rows.into_iter().next())
}

pub(super) async fn delete(
    schema: &Schema,
    conn: &Arc<Mutex<Connection>>,
    model: &str,
    pk: &str,
) -> Result<Option<Row>, DataError> {
    let (resolved, info) = resolve_model(schema, model)?;
    let resolved = resolved.clone();
    let sql = build_delete_sql(&info);
    let pk_owned = pk.to_owned();

    let rows = with_conn(conn.clone(), move |conn| {
        match fetch_rows(conn, &sql, &[&pk_owned]) {
            Ok(rows) => Ok(rows),
            Err(DataError::Sqlite(e)) => Err(map_sqlite_error(Some(&resolved), &e)
                .unwrap_or(DataError::Sqlite(e))),
            Err(other) => Err(other),
        }
    })
    .await?;
    Ok(rows.into_iter().next())
}

pub(super) async fn follow(
    schema: &Schema,
    conn: &Arc<Mutex<Connection>>,
    target_model: &str,
    filter_column: &str,
    filter_cast: PkCast,
    filter_value: &str,
    page: PageRequest<'_>,
) -> Result<Page, DataError> {
    let (resolved_model, info) = resolve_model(schema, target_model)?;
    let limit = clamp_limit(page.limit);
    let sql = build_list_on_column_sql(&info, filter_column, filter_cast, limit);
    let pk_field_name = find_pk_field(resolved_model)
        .map(|f| f.name.clone())
        .expect("resolve_model returns an error when there is no @id");
    let filter_owned = filter_value.to_owned();
    let cursor_owned = page.cursor.map(str::to_owned);

    let rows = with_conn(conn.clone(), move |conn| match cursor_owned {
        Some(c) => fetch_rows(conn, &sql, &[&filter_owned, &c]),
        None => fetch_rows(conn, &sql, &[&filter_owned, &rusqlite::types::Null]),
    })
    .await?;

    let next_cursor = next_cursor(&rows, &pk_field_name, limit);
    Ok(Page { rows, next_cursor })
}

