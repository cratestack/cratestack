//! SQLite-backed `ClientStateStore` for `cratestack-client-rust`.

mod bootstrap;
mod ops;
mod store;

pub use store::SqliteStateStore;

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
