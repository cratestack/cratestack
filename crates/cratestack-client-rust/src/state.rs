use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::ClientError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestJournalEntry {
    pub method: String,
    pub path: String,
    pub status_code: u16,
    pub content_type: Option<String>,
    pub recorded_at: DateTime<Utc>,
}

fn default_schema_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedClientState {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub state_version: u64,
    #[serde(default)]
    pub request_journal: Vec<RequestJournalEntry>,
}

impl Default for PersistedClientState {
    fn default() -> Self {
        Self {
            schema_version: default_schema_version(),
            state_version: 0,
            request_journal: Vec::new(),
        }
    }
}

pub trait ClientStateStore: Send + Sync {
    fn load(&self) -> Result<PersistedClientState, ClientError>;
    fn save(&self, state: &PersistedClientState) -> Result<(), ClientError>;

    fn append_request_journal(&self, entry: &RequestJournalEntry) -> Result<(), ClientError> {
        let mut state = self.load()?;
        state.request_journal.push(entry.clone());
        state.state_version = state.state_version.saturating_add(1);
        self.save(&state)
    }
}

#[derive(Debug, Default)]
pub struct InMemoryStateStore {
    state: Mutex<PersistedClientState>,
}

impl ClientStateStore for InMemoryStateStore {
    fn load(&self) -> Result<PersistedClientState, ClientError> {
        self.state
            .lock()
            .map_err(|error| ClientError::State(format!("failed to lock state store: {error}")))
            .map(|state| state.clone())
    }

    fn save(&self, state: &PersistedClientState) -> Result<(), ClientError> {
        let mut guard = self
            .state
            .lock()
            .map_err(|error| ClientError::State(format!("failed to lock state store: {error}")))?;
        *guard = state.clone();
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct JsonFileStateStore {
    path: PathBuf,
}

impl JsonFileStateStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl ClientStateStore for JsonFileStateStore {
    fn load(&self) -> Result<PersistedClientState, ClientError> {
        match fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
                ClientError::State(format!(
                    "failed to decode state file {}: {error}",
                    self.path.display()
                ))
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(PersistedClientState::default())
            }
            Err(error) => Err(ClientError::State(format!(
                "failed to read state file {}: {error}",
                self.path.display()
            ))),
        }
    }

    fn save(&self, state: &PersistedClientState) -> Result<(), ClientError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                ClientError::State(format!(
                    "failed to create state directory {}: {error}",
                    parent.display()
                ))
            })?;
        }
        let bytes = serde_json::to_vec_pretty(state).map_err(|error| {
            ClientError::State(format!(
                "failed to encode state file {}: {error}",
                self.path.display()
            ))
        })?;
        fs::write(&self.path, bytes).map_err(|error| {
            ClientError::State(format!(
                "failed to write state file {}: {error}",
                self.path.display()
            ))
        })
    }
}
