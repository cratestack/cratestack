use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use cratestack_client_rust::{
    ClientError, ClientStateStore, PersistedClientState, RequestJournalEntry,
};
use rusqlite::{Connection, OptionalExtension, params};

const SQLITE_SCHEMA_VERSION: u32 = 1;

pub struct SqliteStateStore {
    connection: Mutex<Connection>,
    path: PathBuf,
}

impl SqliteStateStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, ClientError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                ClientError::State(format!(
                    "failed to create SQLite state directory {}: {error}",
                    parent.display()
                ))
            })?;
        }
        let connection = Connection::open(&path).map_err(|error| {
            ClientError::State(format!(
                "failed to open SQLite state store {}: {error}",
                path.display()
            ))
        })?;
        bootstrap(&connection, SQLITE_SCHEMA_VERSION)?;

        Ok(Self {
            connection: Mutex::new(connection),
            path,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl ClientStateStore for SqliteStateStore {
    fn load(&self) -> Result<PersistedClientState, ClientError> {
        let connection = self.connection.lock().map_err(|error| {
            ClientError::State(format!("failed to lock SQLite state store: {error}"))
        })?;
        load_state(&connection)
    }

    fn save(&self, state: &PersistedClientState) -> Result<(), ClientError> {
        let mut connection = self.connection.lock().map_err(|error| {
            ClientError::State(format!("failed to lock SQLite state store: {error}"))
        })?;
        save_state(&mut connection, state)
    }

    fn append_request_journal(&self, entry: &RequestJournalEntry) -> Result<(), ClientError> {
        let mut connection = self.connection.lock().map_err(|error| {
            ClientError::State(format!("failed to lock SQLite state store: {error}"))
        })?;
        let transaction = connection.transaction().map_err(sqlite_error)?;
        transaction
            .execute(
                "INSERT INTO request_journal (method, path, status_code, content_type, recorded_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    &entry.method,
                    &entry.path,
                    entry.status_code,
                    &entry.content_type,
                    entry.recorded_at.to_rfc3339(),
                ],
            )
            .map_err(sqlite_error)?;
        transaction
            .execute(
                "UPDATE state_meta SET state_version = state_version + 1, updated_at = ?1 WHERE singleton = 1",
                params![Utc::now().to_rfc3339()],
            )
            .map_err(sqlite_error)?;
        transaction.commit().map_err(sqlite_error)
    }
}

fn bootstrap(connection: &Connection, schema_version: u32) -> Result<(), ClientError> {
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

fn load_state(connection: &Connection) -> Result<PersistedClientState, ClientError> {
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

fn save_state(
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

fn sqlite_error(error: rusqlite::Error) -> ClientError {
    ClientError::State(format!("SQLite state store error: {error}"))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use chrono::Utc;
    use cratestack_client_rust::{ClientStateStore, RequestJournalEntry};

    use super::SqliteStateStore;

    #[test]
    fn bootstrap_loads_default_state() {
        let path = project_tmp_path("bootstrap");
        cleanup(&path);

        let store = SqliteStateStore::open(&path).expect("store should open");
        let state = store.load().expect("state should load");

        assert_eq!(state.schema_version, 1);
        assert_eq!(state.state_version, 0);
        assert!(state.request_journal.is_empty());

        cleanup(&path);
    }

    #[test]
    fn append_round_trips_and_increments_state_version() {
        let path = project_tmp_path("append");
        cleanup(&path);

        let store = SqliteStateStore::open(&path).expect("store should open");
        store
            .append_request_journal(&RequestJournalEntry {
                method: "POST".to_owned(),
                path: "/$procs/getFeed".to_owned(),
                status_code: 200,
                content_type: Some("application/cbor".to_owned()),
                recorded_at: Utc::now(),
            })
            .expect("journal entry should append");

        let state = store.load().expect("state should load");
        assert_eq!(state.state_version, 1);
        assert_eq!(state.request_journal.len(), 1);

        cleanup(&path);
    }

    fn project_tmp_path(label: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp/client-store-sqlite-tests")
            .join(format!("{label}-{suffix}.sqlite"))
    }

    fn cleanup(path: &Path) {
        if path.exists() {
            std::fs::remove_file(path).expect("tmp file should be removable");
        }
    }
}
