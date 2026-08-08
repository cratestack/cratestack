//! `ClientStateStore` trait and companion types for client-side state persistence.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::CoolError;

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
    fn load(&self) -> Result<PersistedClientState, CoolError>;
    fn save(&self, state: &PersistedClientState) -> Result<(), CoolError>;

    fn append_request_journal(&self, entry: &RequestJournalEntry) -> Result<(), CoolError> {
        let mut state = self.load()?;
        state.request_journal.push(entry.clone());
        state.state_version = state.state_version.saturating_add(1);
        self.save(&state)
    }
}

#[derive(Debug, Default)]
pub struct InMemoryStateStore {
    state: std::sync::Mutex<PersistedClientState>,
}

impl ClientStateStore for InMemoryStateStore {
    fn load(&self) -> Result<PersistedClientState, CoolError> {
        self.state
            .lock()
            .map_err(|error| CoolError::Internal(format!("failed to lock state store: {error}")))
            .map(|state| state.clone())
    }

    fn save(&self, state: &PersistedClientState) -> Result<(), CoolError> {
        let mut guard = self
            .state
            .lock()
            .map_err(|error| CoolError::Internal(format!("failed to lock state store: {error}")))?;
        *guard = state.clone();
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct JsonFileStateStore {
    path: std::path::PathBuf,
}

impl JsonFileStateStore {
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl ClientStateStore for JsonFileStateStore {
    fn load(&self) -> Result<PersistedClientState, CoolError> {
        match std::fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
                CoolError::Internal(format!(
                    "failed to decode state file {}: {error}",
                    self.path.display()
                ))
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(PersistedClientState::default())
            }
            Err(error) => Err(CoolError::Internal(format!(
                "failed to read state file {}: {error}",
                self.path.display()
            ))),
        }
    }

    fn save(&self, state: &PersistedClientState) -> Result<(), CoolError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                CoolError::Internal(format!(
                    "failed to create state directory {}: {error}",
                    parent.display()
                ))
            })?;
        }
        let bytes = serde_json::to_vec_pretty(state).map_err(|error| {
            CoolError::Internal(format!(
                "failed to encode state file {}: {error}",
                self.path.display()
            ))
        })?;
        std::fs::write(&self.path, bytes).map_err(|error| {
            CoolError::Internal(format!(
                "failed to write state file {}: {error}",
                self.path.display()
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_persisted_client_state() {
        let state = PersistedClientState::default();
        assert_eq!(state.schema_version, 1);
        assert_eq!(state.state_version, 0);
        assert!(state.request_journal.is_empty());
    }

    #[test]
    fn in_memory_store_round_trips() {
        let store = InMemoryStateStore::default();
        let state = PersistedClientState {
            schema_version: 1,
            state_version: 1,
            request_journal: vec![RequestJournalEntry {
                method: "POST".to_owned(),
                path: "/$procs/test".to_owned(),
                status_code: 200,
                content_type: Some("application/cbor".to_owned()),
                recorded_at: Utc::now(),
            }],
        };
        store.save(&state).expect("save should work");
        let loaded = store.load().expect("load should work");
        assert_eq!(loaded.state_version, state.state_version);
        assert_eq!(loaded.request_journal.len(), 1);
    }
}
