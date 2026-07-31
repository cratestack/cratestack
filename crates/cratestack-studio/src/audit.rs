//! Audit ring buffer for Studio writes.
//!
//! Every successful CREATE / UPDATE / DELETE Studio performs is
//! captured here. The read path is always an in-memory ring capped at
//! [`AuditLog::CAPACITY`] with FIFO eviction, so `GET /api/audit` stays
//! bounded regardless of how long Studio has been running.
//!
//! By default that ring is *all* there is: Studio is a local admin
//! tool with a zero-footprint promise, and a process-lifetime buffer
//! costs the operator nothing. Setting `[workspace] audit_file` in
//! `studio.toml` additionally mirrors each entry to an append-only
//! JSONL sidecar and replays it on boot, so the log survives restarts.
//! That sink is Studio-local by construction — see [`store`] for why
//! it is never a table in the target database.

mod store;
mod time;

#[cfg(test)]
mod tests;

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

pub use store::{AuditStore, AuditStoreError};

/// One captured write. `at` is an RFC-3339 timestamp; `pk` is the
/// row's primary-key value after the write (so for CREATE we capture
/// the generated value if the DB filled one in).
///
/// `Deserialize` is implemented because entries round-trip through the
/// JSONL sink; it is not part of any request-parsing surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: u64,
    pub at: String,
    pub target: String,
    pub model: String,
    pub op: AuditOp,
    pub pk: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum AuditOp {
    Create,
    Update,
    Delete,
}

/// Ring contents and the id counter under one lock.
///
/// They are deliberately not separate mutexes: ids are allocated in the
/// same critical section that appends to the ring and to the file, so
/// the on-disk line order always matches id order even under concurrent
/// requests.
#[derive(Debug)]
struct State {
    entries: VecDeque<AuditEntry>,
    next_id: u64,
}

#[derive(Debug)]
pub struct AuditLog {
    state: Mutex<State>,
    store: Option<AuditStore>,
}

impl AuditLog {
    pub const CAPACITY: usize = 500;

    /// In-memory only — the default, and what every target gets when
    /// `studio.toml` doesn't ask for persistence.
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State {
                entries: VecDeque::with_capacity(Self::CAPACITY),
                next_id: 1,
            }),
            store: None,
        }
    }

    /// Open (creating if needed) the JSONL sink at `path`, replay its
    /// tail into the ring, and mirror future entries to it.
    ///
    /// Errors are propagated rather than swallowed: the operator asked
    /// for persistence explicitly, so booting with a silently inert
    /// sink would misrepresent what Studio is doing.
    pub fn persistent(path: &Path) -> Result<Self, AuditStoreError> {
        let (store, replay) = AuditStore::open(path, Self::CAPACITY)?;
        if replay.skipped_lines > 0 {
            tracing::warn!(
                path = %path.display(),
                skipped = replay.skipped_lines,
                "skipped unparseable lines while replaying the audit log"
            );
        }
        Ok(Self {
            state: Mutex::new(State {
                // Resume past the highest id in the *whole* file, not
                // just the replayed tail, so ids stay unique across
                // restarts even when the cap dropped older entries.
                next_id: replay.max_id + 1,
                entries: replay.entries.into(),
            }),
            store: Some(store),
        })
    }

    pub fn push(&self, target: &str, model: &str, op: AuditOp, pk: Option<String>) {
        let mut state = self.state.lock().expect("audit mutex poisoned");
        let entry = AuditEntry {
            id: state.next_id,
            at: time::now_rfc3339(),
            target: target.to_owned(),
            model: model.to_owned(),
            op,
            pk,
        };
        state.next_id += 1;
        if let Some(store) = &self.store {
            store.append(&entry);
        }
        if state.entries.len() == Self::CAPACITY {
            state.entries.pop_front();
        }
        state.entries.push_back(entry);
    }

    /// Snapshot the most recent `limit` entries in reverse-chronological
    /// order (newest first). `limit` is clamped to the buffer capacity.
    pub fn snapshot(&self, limit: usize) -> Vec<AuditEntry> {
        let state = self.state.lock().expect("audit mutex poisoned");
        let limit = limit.min(state.entries.len());
        state.entries.iter().rev().take(limit).cloned().collect()
    }
}

impl Default for AuditLog {
    fn default() -> Self {
        Self::new()
    }
}
