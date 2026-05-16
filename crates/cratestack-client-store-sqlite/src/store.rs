//! The `SqliteStateStore` handle and `ClientStateStore` impl.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::Utc;
use cratestack_client_rust::{
    ClientError, ClientStateStore, PersistedClientState, RequestJournalEntry,
};
use rusqlite::{Connection, params};

use crate::bootstrap::{SQLITE_SCHEMA_VERSION, bootstrap, sqlite_error};
use crate::ops::{load_state, save_state};

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
