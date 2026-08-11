//! HTTP-level tests for cratestack#507: a `[target.db]` target writes
//! straight to SQL, bypassing `@version` bumping and `@@emit` outbox
//! rows. Studio now refuses to write `@version`/`@@emit` models on a
//! `rw` `[target.db]` target unless the target set
//! `allow_unsafe_writes = true`, so the bypass is chosen per target
//! rather than discovered after the fact.
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
    assert!(message.contains("@version"), "{message}");
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
