//! Schema bootstrap + shared error mapping for the SQLite state store.

use chrono::Utc;
use cratestack_client_rust::ClientError;
use rusqlite::{Connection, OptionalExtension, params};

pub(crate) const SQLITE_SCHEMA_VERSION: u32 = 1;

pub(crate) fn bootstrap(connection: &Connection, schema_version: u32) -> Result<(), ClientError> {
    connection
        .execute_batch(
            "
            CREATE TABLE IF NOT EXISTS state_meta (
              singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
              schema_version INTEGER NOT NULL,
              state_version INTEGER NOT NULL,
              updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS request_journal (
              seq INTEGER PRIMARY KEY AUTOINCREMENT,
              method TEXT NOT NULL,
              path TEXT NOT NULL,
              status_code INTEGER NOT NULL,
              content_type TEXT,
              recorded_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS request_journal_recorded_at_idx
              ON request_journal(recorded_at);
            ",
        )
        .map_err(sqlite_error)?;
    let exists = connection
        .query_row("SELECT 1 FROM state_meta WHERE singleton = 1", [], |row| {
            row.get::<_, i64>(0)
        })
        .optional()
        .map_err(sqlite_error)?
        .is_some();
    if !exists {
        connection
            .execute(
                "INSERT INTO state_meta (singleton, schema_version, state_version, updated_at) VALUES (1, ?1, 0, ?2)",
                params![schema_version, Utc::now().to_rfc3339()],
            )
            .map_err(sqlite_error)?;
    }
    Ok(())
}

pub(crate) fn sqlite_error(error: rusqlite::Error) -> ClientError {
    ClientError::State(format!("SQLite state store error: {error}"))
}
