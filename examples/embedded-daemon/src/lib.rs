//! Library half of `embedded-daemon`. Holds the bits that are unit-testable
//! without spinning up a tokio runtime or a real filesystem watcher:
//!
//! - [`include_embedded_schema!`] for the `FileEvent` model.
//! - [`bootstrap`] and [`persist_event`] — the sync persistence boundary that
//!   `main.rs` calls through `tokio::task::spawn_blocking`.
//! - [`Debouncer`] — pure state machine that collapses bursty filesystem
//!   events into one row per `(path, settle_window)` pair.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use cratestack_macros::include_embedded_schema;
use cratestack_rusqlite::ddl::create_table_sql;
use cratestack_rusqlite::{ModelDelegate, RusqliteError, RusqliteRuntime};
use uuid::Uuid;

include_embedded_schema!("schema.cstack");

pub use cratestack_schema::{CreateFileEventInput, FileEvent};

pub fn bootstrap(runtime: &RusqliteRuntime) -> Result<(), RusqliteError> {
    runtime.with_connection(|conn| {
        conn.execute_batch(&create_table_sql(&cratestack_schema::FILE_EVENT_MODEL))?;
        Ok(())
    })
}

/// A single debounced filesystem event, ready to be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadyEvent {
    pub path: PathBuf,
    pub kind: String,
    pub observed_at: DateTime<Utc>,
    pub bursts: i64,
}

pub fn persist_event(
    runtime: &RusqliteRuntime,
    event: ReadyEvent,
) -> Result<FileEvent, RusqliteError> {
    let events = ModelDelegate::new(runtime, &cratestack_schema::FILE_EVENT_MODEL);
    events
        .create(CreateFileEventInput {
            id: Uuid::new_v4(),
            path: event.path.to_string_lossy().into_owned(),
            kind: event.kind,
            observedAt: event.observed_at,
            bursts: event.bursts,
        })
        .run()
}

/// State for collapsing bursts of raw fs events into one row per (path, window).
///
/// Each path keeps a `PendingEvent` whose `last_seen` advances as more events
/// for that path arrive. When the gap between `last_seen` and `now` exceeds
/// `window`, the entry is "ready" and gets drained.
#[derive(Debug)]
pub struct Debouncer {
    window: Duration,
    pending: HashMap<PathBuf, PendingEvent>,
}

#[derive(Debug)]
struct PendingEvent {
    kind: String,
    last_seen: Instant,
    bursts: i64,
}

impl Debouncer {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            pending: HashMap::new(),
        }
    }

    /// Record one raw event. If a `delete` lands after a `create`/`modify`
    /// for the same path, the resulting row will be a single `deleted` —
    /// the delete supersedes whatever was buffered.
    pub fn observe(&mut self, path: PathBuf, kind: &str, now: Instant) {
        match self.pending.get_mut(&path) {
            Some(entry) => {
                entry.last_seen = now;
                entry.bursts += 1;
                if supersedes(&entry.kind, kind) {
                    entry.kind = kind.to_owned();
                }
            }
            None => {
                self.pending.insert(
                    path,
                    PendingEvent {
                        kind: kind.to_owned(),
                        last_seen: now,
                        bursts: 1,
                    },
                );
            }
        }
    }

    /// Drain every entry that hasn't been touched within `window`. Stamps
    /// the produced rows with the `wall` clock so tests don't need to fake
    /// `chrono::Utc::now`.
    pub fn drain_ready(&mut self, now: Instant, wall: DateTime<Utc>) -> Vec<ReadyEvent> {
        let window = self.window;
        let ready_paths: Vec<PathBuf> = self
            .pending
            .iter()
            .filter(|(_, entry)| now.duration_since(entry.last_seen) >= window)
            .map(|(path, _)| path.clone())
            .collect();

        ready_paths
            .into_iter()
            .map(|path| {
                let entry = self.pending.remove(&path).expect("ready entry must exist");
                ReadyEvent {
                    path,
                    kind: entry.kind,
                    observed_at: wall,
                    bursts: entry.bursts,
                }
            })
            .collect()
    }

    /// Flush every pending entry regardless of age. Used on shutdown.
    pub fn drain_all(&mut self, wall: DateTime<Utc>) -> Vec<ReadyEvent> {
        self.pending
            .drain()
            .map(|(path, entry)| ReadyEvent {
                path,
                kind: entry.kind,
                observed_at: wall,
                bursts: entry.bursts,
            })
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

fn supersedes(existing: &str, incoming: &str) -> bool {
    // delete always wins; otherwise the most recent kind wins.
    match (existing, incoming) {
        ("deleted", _) => false,
        (_, "deleted") => true,
        _ => existing != incoming,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn burst_of_modifies_collapses_to_one_row() {
        let base = Instant::now();
        let mut deb = Debouncer::new(Duration::from_millis(100));

        for offset in [0, 10, 30, 60, 80] {
            deb.observe(PathBuf::from("/tmp/a"), "modified", ts(base, offset));
        }
        // before the window has elapsed, nothing is ready
        assert!(deb.drain_ready(ts(base, 90), Utc::now()).is_empty());

        let ready = deb.drain_ready(ts(base, 250), Utc::now());
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].path, PathBuf::from("/tmp/a"));
        assert_eq!(ready[0].kind, "modified");
        assert_eq!(ready[0].bursts, 5);
    }

    #[test]
    fn delete_after_modify_wins() {
        let base = Instant::now();
        let mut deb = Debouncer::new(Duration::from_millis(50));
        deb.observe(PathBuf::from("/tmp/a"), "created", base);
        deb.observe(PathBuf::from("/tmp/a"), "modified", ts(base, 10));
        deb.observe(PathBuf::from("/tmp/a"), "deleted", ts(base, 20));
        let ready = deb.drain_ready(ts(base, 200), Utc::now());
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].kind, "deleted");
        assert_eq!(ready[0].bursts, 3);
    }

    #[test]
    fn different_paths_drain_independently() {
        let base = Instant::now();
        let mut deb = Debouncer::new(Duration::from_millis(50));
        deb.observe(PathBuf::from("/tmp/a"), "modified", base);
        deb.observe(PathBuf::from("/tmp/b"), "modified", ts(base, 60));

        let ready = deb.drain_ready(ts(base, 70), Utc::now());
        assert_eq!(ready.len(), 1, "only /tmp/a should be drained");
        assert_eq!(ready[0].path, PathBuf::from("/tmp/a"));
        assert_eq!(deb.is_empty(), false);

        let ready = deb.drain_ready(ts(base, 200), Utc::now());
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].path, PathBuf::from("/tmp/b"));
        assert!(deb.is_empty());
    }

    #[test]
    fn persist_round_trip_against_in_memory_db() {
        let runtime = RusqliteRuntime::open_in_memory().expect("open in-memory");
        bootstrap(&runtime).expect("bootstrap");

        let event = ReadyEvent {
            path: PathBuf::from("/var/log/example.log"),
            kind: "modified".into(),
            observed_at: Utc::now(),
            bursts: 3,
        };
        let view = persist_event(&runtime, event.clone()).expect("persist");
        assert_eq!(view.path, "/var/log/example.log");
        assert_eq!(view.kind, "modified");
        assert_eq!(view.bursts, 3);

        let events = ModelDelegate::new(&runtime, &cratestack_schema::FILE_EVENT_MODEL);
        let rows = events.find_many().run().expect("find_many");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, view.id);
    }
}
