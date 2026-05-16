use std::path::PathBuf;

use super::{
    ClientStateStore, JsonFileStateStore, PersistedClientState, RequestJournalEntry,
    RuntimeCodecConfig, RuntimeConfigWire, RuntimeEnvelopeConfig, RuntimeErrorCode,
    RuntimeHandle, RuntimeRequestWire, RuntimeStateStoreConfig, RuntimeTransportConfig,
};

#[test]
fn json_file_store_round_trips_state_under_project_tmp() {
    let path = project_tmp_path("state-store-unit.json");
    if path.exists() {
        std::fs::remove_file(&path).expect("existing tmp file should be removable");
    }

    let store = JsonFileStateStore::new(&path);
    store
        .append_request_journal(&RequestJournalEntry {
            method: "GET".to_owned(),
            path: "/posts".to_owned(),
            status_code: 200,
            content_type: Some("application/cbor".to_owned()),
            recorded_at: chrono::Utc::now(),
        })
        .expect("journal entry should append");

    let loaded = store.load().expect("state should load");
    assert_eq!(loaded.schema_version, 1);
    assert_eq!(loaded.state_version, 1);
    assert_eq!(loaded.request_journal.len(), 1);

    std::fs::remove_file(&path).expect("tmp file should be removable");
}

#[test]
fn runtime_handle_rejects_invalid_method_without_running_http() {
    let handle = RuntimeHandle::new(RuntimeConfigWire {
        base_url: "http://127.0.0.1:1/".to_owned(),
        state_store: RuntimeStateStoreConfig::InMemory,
        transport: RuntimeTransportConfig::default(),
    })
    .expect("runtime handle should build");

    let error = handle
        .execute(RuntimeRequestWire {
            method: "BAD METHOD".to_owned(),
            path: "/posts".to_owned(),
            canonical_query: None,
            headers: Vec::new(),
            body: Vec::new(),
        })
        .expect_err("invalid method should fail before transport");

    assert_eq!(error.code as u32, super::RuntimeErrorCode::BadInput as u32);
}

#[test]
fn persisted_state_defaults_missing_state_version() {
    let state: PersistedClientState =
        serde_json::from_str(r#"{"schema_version":1,"request_journal":[]}"#)
            .expect("legacy state should decode");

    assert_eq!(state.state_version, 0);
}

#[test]
fn runtime_handle_rejects_unsupported_envelope_config() {
    let result = RuntimeHandle::new(RuntimeConfigWire {
        base_url: "http://127.0.0.1:1/".to_owned(),
        state_store: RuntimeStateStoreConfig::InMemory,
        transport: RuntimeTransportConfig {
            codec: RuntimeCodecConfig::Cbor,
            envelope: RuntimeEnvelopeConfig::CoseSign1,
        },
    });

    let error = match result {
        Ok(_) => panic!("unsupported envelope should fail"),
        Err(error) => error,
    };

    assert_eq!(error.code, RuntimeErrorCode::BadInput);
}

fn project_tmp_path(file_name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tmp/client-rust-tests")
        .join(file_name)
}
