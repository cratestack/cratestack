//! Load and save operations for the SQLite state store.

use chrono::{DateTime, Utc};
use cratestack_client_rust::{ClientError, PersistedClientState, RequestJournalEntry};
use rusqlite::{Connection, params};

use crate::bootstrap::sqlite_error;

pub(crate) fn load_state(connection: &Connection) -> Result<PersistedClientState, ClientError> {
    let (schema_version, state_version) = connection
        .query_row(
            "SELECT schema_version, state_version FROM state_meta WHERE singleton = 1",
            [],
            |row| Ok((row.get::<_, u32>(0)?, row.get::<_, u64>(1)?)),
        )
        .map_err(sqlite_error)?;

    let mut statement = connection
        .prepare(
            "SELECT method, path, status_code, content_type, recorded_at FROM request_journal ORDER BY seq ASC",
        )
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map([], |row| {
            let recorded_at = row.get::<_, String>(4)?;
            let recorded_at = DateTime::parse_from_rfc3339(&recorded_at)
                .map(|value| value.with_timezone(&Utc))
                .map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        4,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?;
            Ok(RequestJournalEntry {
                method: row.get(0)?,
                path: row.get(1)?,
                status_code: row.get(2)?,
                content_type: row.get(3)?,
                recorded_at,
            })
        })
        .map_err(sqlite_error)?;
    let request_journal = rows.collect::<Result<Vec<_>, _>>().map_err(sqlite_error)?;

    Ok(PersistedClientState {
        schema_version,
        state_version,
        request_journal,
    })
}

pub(crate) fn save_state(
    connection: &mut Connection,
    state: &PersistedClientState,
) -> Result<(), ClientError> {
    let transaction = connection.transaction().map_err(sqlite_error)?;
    transaction
        .execute("DELETE FROM request_journal", [])
        .map_err(sqlite_error)?;
    transaction
        .execute(
            "UPDATE state_meta SET schema_version = ?1, state_version = ?2, updated_at = ?3 WHERE singleton = 1",
            params![state.schema_version, state.state_version, Utc::now().to_rfc3339()],
        )
        .map_err(sqlite_error)?;

    {
        let mut statement = transaction
            .prepare(
                "INSERT INTO request_journal (method, path, status_code, content_type, recorded_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .map_err(sqlite_error)?;
        for entry in &state.request_journal {
            statement
                .execute(params![
                    &entry.method,
                    &entry.path,
                    entry.status_code,
                    &entry.content_type,
                    entry.recorded_at.to_rfc3339(),
                ])
                .map_err(sqlite_error)?;
        }
    }

    transaction.commit().map_err(sqlite_error)
}
