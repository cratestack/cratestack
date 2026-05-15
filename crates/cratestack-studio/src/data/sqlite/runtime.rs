//! Async ⇄ blocking bridge for rusqlite.
//!
//! rusqlite is synchronous; every query runs through
//! [`tokio::task::spawn_blocking`] with the connection held behind a
//! [`tokio::sync::Mutex`]. SQLite is single-writer anyway, so the
//! per-source serialization isn't a scaling loss in practice.

use std::sync::Arc;

use rusqlite::Connection;
use tokio::sync::Mutex;

use crate::data::{DataError, Row};

/// Run a closure against a SQLite connection on the blocking pool.
/// The connection is locked for the duration of the closure.
pub(super) async fn with_conn<F, R>(
    connection: Arc<Mutex<Connection>>,
    f: F,
) -> Result<R, DataError>
where
    F: FnOnce(&mut Connection) -> Result<R, DataError> + Send + 'static,
    R: Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let mut conn = connection.blocking_lock();
        f(&mut conn)
    })
    .await
    .map_err(|e| DataError::BlockingJoin(e.to_string()))?
}

/// Project every row's `json_object(...)` text back into a Studio [`Row`].
/// Non-object JSON values (defensive: shouldn't happen with our SQL)
/// are silently dropped.
pub(super) fn fetch_rows(
    conn: &Connection,
    sql: &str,
    params: &[&dyn rusqlite::ToSql],
) -> Result<Vec<Row>, DataError> {
    let mut stmt = conn.prepare(sql)?;
    let mut iter = stmt.query(params)?;
    let mut rows = Vec::new();
    while let Some(row) = iter.next()? {
        let text: String = row.get(0)?;
        let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
            DataError::Sqlite(rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::other(e.to_string())),
            ))
        })?;
        if let serde_json::Value::Object(map) = value {
            rows.push(map);
        }
    }
    Ok(rows)
}
