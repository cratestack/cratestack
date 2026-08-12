//! HTTP-level tests for cratestack#507: a `[target.db]` target writes
//! straight to SQL, bypassing `@version` bumping and `@@emit` outbox
//! rows.
//!
//! As of "option 3" (routing writes through the same primitives the
//! generated server uses — see `crate::api::records::guards::WriteMode`),
//! `@version` bumping is always routable, on every backend, so a model
//! that only declares `@version` is never refused here — see
//! `version_only_model_is_routed_and_bumps_for_real_without_opt_in`
//! below. `@@emit` is different: SQLite-embedded deployments have no
//! `cratestack_event_outbox` equivalent at all, so a model that declares
//! `@@emit(...)` is still refused on a SQLite `[target.db]` target
//! unless it opts into `allow_unsafe_writes = true` — the tests below
//! cover that half of the table. (`tests/postgres_routed_writes.rs`
//! covers the Postgres half, where `@@emit` *is* routable, against a
//! live database.)
//!
//! Two in-memory SQLite `rw` targets share the same schema and seed
//! data: `unsafe_off` (the default — `allow_unsafe_writes` unset) and
//! `unsafe_on` (`allow_unsafe_writes = true`). Neither needs a real
//! Postgres, so this file is not gated on `CRATESTACK_TEST_DATABASE_URL`.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cratestack_studio::audit::AuditLog;
use cratestack_studio::config::{TargetMode, WorkspaceConfig};
use cratestack_studio::data::sqlite::SqliteSource;
use cratestack_studio::workspace::{LoadedTarget, LoadedWorkspace};
use rusqlite::Connection;
use serde_json::Value;
use tower::ServiceExt;

const SCHEMA: &str = r#"
model Message {
  id String @id
  version Int @version
  stateReason String
  @@emit(created, updated, deleted)
}

model Plain {
  id String @id
  name String
}

model VersionOnly {
  id String @id
  version Int @version
  label String
}
"#;

fn seeded_conn() -> Connection {
    let conn = Connection::open_in_memory().expect("sqlite open");
    conn.execute_batch(
        r#"
        CREATE TABLE messages (
          id TEXT PRIMARY KEY,
          version INTEGER NOT NULL,
          state_reason TEXT NOT NULL
        );
        INSERT INTO messages VALUES ('msg1', 0, 'accepted');
        CREATE TABLE plains (
          id TEXT PRIMARY KEY,
          name TEXT NOT NULL
        );
        INSERT INTO plains VALUES ('p1', 'original');
        CREATE TABLE version_onlies (
          id TEXT PRIMARY KEY,
          version INTEGER NOT NULL,
          label TEXT NOT NULL
        );
        INSERT INTO version_onlies VALUES ('v1', 0, 'original');
        "#,
    )
    .expect("ddl");
    conn
}

/// `allow_unsafe_writes` on `unsafe_off` is left at its default
/// (`false`); `unsafe_on` sets it explicitly.
fn build_workspace() -> Arc<LoadedWorkspace> {
    let schema = Arc::new(cratestack_parser::parse_schema(SCHEMA).expect("schema parses"));

    let unsafe_off = LoadedTarget {
        key: "unsafe_off".to_owned(),
        display_name: "Unsafe writes off".to_owned(),
        mode: TargetMode::Rw,
        schema: schema.clone(),
        schema_path: PathBuf::from("schema.cstack"),
        source: Arc::new(SqliteSource::new(seeded_conn(), schema.clone())),
        has_db: true,
        has_api: false,
        allow_unsafe_db_writes: false,
    };

    let unsafe_on = LoadedTarget {
        key: "unsafe_on".to_owned(),
        display_name: "Unsafe writes on".to_owned(),
        mode: TargetMode::Rw,
        schema: schema.clone(),
        schema_path: PathBuf::from("schema.cstack"),
        source: Arc::new(SqliteSource::new(seeded_conn(), schema.clone())),
        has_db: true,
        has_api: false,
        allow_unsafe_db_writes: true,
    };

    Arc::new(LoadedWorkspace {
        config: WorkspaceConfig {
            name: "unsafe-writes-smoke".to_owned(),
            default_mode: TargetMode::Ro,
            cors_dev: true,
            audit_file: None,
        },
        targets: vec![Arc::new(unsafe_off), Arc::new(unsafe_on)],
        audit: Arc::new(AuditLog::new()),
    })
}

async fn json_request(method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let app = cratestack_studio::server::build_router(build_workspace());
    let mut builder = Request::builder().method(method).uri(uri);
    let body_bytes = match body {
        Some(v) => {
            builder = builder.header("content-type", "application/json");
            serde_json::to_vec(&v).expect("body serializes")
        }
        None => Vec::new(),
    };
    let response = app
        .oneshot(builder.body(Body::from(body_bytes)).unwrap())
        .await
        .expect("request");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body");
    let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

#[tokio::test]
async fn update_versioned_emitting_model_without_opt_in_is_refused() {
    let (status, body) = json_request(
        "PATCH",
        "/api/targets/unsafe_off/models/Message/records/msg1",
        Some(serde_json::json!({ "stateReason": "written by studio" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "UNSAFE_DB_WRITE");
    let message = body["error"]["message"].as_str().expect("message string");
    // `@version` alone is always routable (cratestack#507 "option 3"), so
    // it's no longer named in the refusal — only the annotation that
    // actually can't be routed on this (SQLite) backend is.
    assert!(message.contains("@@emit"), "{message}");
    assert!(message.contains("allow_unsafe_writes"), "{message}");
}

#[tokio::test]
async fn create_versioned_emitting_model_without_opt_in_is_refused() {
    let (status, body) = json_request(
        "POST",
        "/api/targets/unsafe_off/models/Message/records",
        Some(serde_json::json!({
            "id": "msg2",
            "version": 0,
            "stateReason": "new"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "UNSAFE_DB_WRITE");
}

#[tokio::test]
async fn delete_versioned_emitting_model_without_opt_in_is_refused() {
    let (status, body) = json_request(
        "DELETE",
        "/api/targets/unsafe_off/models/Message/records/msg1",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "UNSAFE_DB_WRITE");
}

#[tokio::test]
async fn update_versioned_emitting_model_with_opt_in_succeeds() {
    let (status, body) = json_request(
        "PATCH",
        "/api/targets/unsafe_on/models/Message/records/msg1",
        Some(serde_json::json!({ "stateReason": "written by studio" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["row"]["stateReason"], "written by studio");
    // The bypass is real even when opted into — this documents (and
    // regression-tests) that opting in does not retroactively make
    // Studio bump `@version`; it only chooses to allow the write.
    assert_eq!(body["row"]["version"], 0);
}

#[tokio::test]
async fn create_versioned_emitting_model_with_opt_in_succeeds() {
    let (status, body) = json_request(
        "POST",
        "/api/targets/unsafe_on/models/Message/records",
        Some(serde_json::json!({
            "id": "msg2",
            "version": 0,
            "stateReason": "new"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["row"]["id"], "msg2");
}

#[tokio::test]
async fn delete_versioned_emitting_model_with_opt_in_succeeds() {
    let (status, body) = json_request(
        "DELETE",
        "/api/targets/unsafe_on/models/Message/records/msg1",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["row"]["id"], "msg1");
}

/// A model with neither `@version` nor `@@emit` is unaffected by the
/// gate even on a target that has not opted into `allow_unsafe_writes`.
#[tokio::test]
async fn model_without_version_or_emit_is_unaffected() {
    let (status, body) = json_request(
        "PATCH",
        "/api/targets/unsafe_off/models/Plain/records/p1",
        Some(serde_json::json!({ "name": "updated" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["row"]["name"], "updated");
}

/// cratestack#507 "option 3": a model that declares only `@version` (no
/// `@@emit`) is never refused, on either `unsafe_off` or `unsafe_on` —
/// there's nothing for `allow_unsafe_writes` to gate, since `@version`
/// bumping is always routable — and the write actually bumps the
/// column, unlike the pre-#507 bypass.
#[tokio::test]
async fn version_only_model_is_routed_and_bumps_for_real_without_opt_in() {
    let (status, body) = json_request(
        "PATCH",
        "/api/targets/unsafe_off/models/VersionOnly/records/v1",
        Some(serde_json::json!({ "label": "written by studio" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["row"]["label"], "written by studio");
    assert_eq!(
        body["row"]["version"], 1,
        "a @version-only model must have its version bumped for real, \
         with no allow_unsafe_writes opt-in required: {body}"
    );
}
