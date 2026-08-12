//! Confirms the two durable signals `require_safe_write`'s bypass path
//! is supposed to leave behind (cratestack#507 finding 3): a
//! `tracing::warn!` naming the target/model/skipped annotations, and an
//! `AuditEntry::unsafe_write` flag reachable through `GET /api/audit`.
//! Both exist so an operator investigating "why didn't @@emit fire for
//! this row" finds something, instead of the exact silence the original
//! issue reported — one config line upstream of it.
//!
//! This is a dedicated integration test binary (rather than unit tests
//! alongside `guards.rs`'s other coverage) so it owns tracing's
//! process-wide callsite Interest cache for its own binary: tracing
//! permanently disables a callsite the first time it is hit with no
//! subscriber attached, so a unit test sharing a binary with ~90 others
//! that may exercise the same call site without a subscriber first
//! would be flaky by construction. `cratestack-pg/tests/include_schema.rs`
//! documents and works around the same issue (#417); this file uses the
//! same `set_global_default` + `Once` shape, simplified because these
//! tests are synchronous (`#[test]`, not `#[tokio::test]`) so a plain
//! thread-local-free global capture is enough — each `cargo test` worker
//! thread runs one test to completion before starting another, so
//! captured lines never interleave mid-assertion, and every test here
//! uses a target/model key unique to itself so concurrent tests can't
//! be mistaken for one another even though they share the capture `Vec`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cratestack_studio::audit::AuditLog;
use cratestack_studio::config::{TargetMode, WorkspaceConfig};
use cratestack_studio::data::sqlite::SqliteSource;
use cratestack_studio::workspace::{LoadedTarget, LoadedWorkspace};
use rusqlite::Connection;
use serde_json::Value;
use tower::ServiceExt;
use tracing::field::{Field, Visit};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::prelude::*;

fn captured_warnings() -> &'static Mutex<Vec<String>> {
    static CAPTURED: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    CAPTURED.get_or_init(|| Mutex::new(Vec::new()))
}

#[derive(Default)]
struct LineVisitor(String);

impl Visit for LineVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.push_str(&format!("{}={value:?} ", field.name()));
    }
}

struct CaptureWarnLayer;

impl<S> Layer<S> for CaptureWarnLayer
where
    S: tracing::Subscriber,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        if *event.metadata().level() != tracing::Level::WARN {
            return;
        }
        let mut visitor = LineVisitor::default();
        event.record(&mut visitor);
        captured_warnings()
            .lock()
            .expect("capture mutex poisoned")
            .push(visitor.0);
    }
}

static TRACING_INIT: std::sync::Once = std::sync::Once::new();

fn init_tracing() {
    TRACING_INIT.call_once(|| {
        let subscriber = tracing_subscriber::registry().with(CaptureWarnLayer);
        tracing::subscriber::set_global_default(subscriber)
            .expect("global tracing subscriber should only be installed once");
    });
}

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

fn build_workspace(probe_target: &str) -> Arc<LoadedWorkspace> {
    let schema = Arc::new(cratestack_parser::parse_schema(SCHEMA).expect("schema parses"));
    let target = LoadedTarget {
        key: probe_target.to_owned(),
        display_name: probe_target.to_owned(),
        mode: TargetMode::Rw,
        schema: schema.clone(),
        schema_path: PathBuf::from("schema.cstack"),
        source: Arc::new(SqliteSource::new(seeded_conn(), schema)),
        has_db: true,
        has_api: false,
        allow_unsafe_db_writes: true,
    };
    Arc::new(LoadedWorkspace {
        config: WorkspaceConfig {
            name: "unsafe-write-signals".to_owned(),
            default_mode: TargetMode::Ro,
            cors_dev: true,
            audit_file: None,
        },
        targets: vec![Arc::new(target)],
        audit: Arc::new(AuditLog::new()),
    })
}

async fn patch(workspace: Arc<LoadedWorkspace>, probe_target: &str) -> (StatusCode, Value) {
    let app = cratestack_studio::server::build_router(workspace);
    let response = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!(
                    "/api/targets/{probe_target}/models/Message/records/msg1"
                ))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({ "stateReason": "written by studio" }))
                        .unwrap(),
                ))
                .unwrap(),
        )
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
async fn bypass_write_logs_a_warning_naming_target_model_and_annotations() {
    init_tracing();
    let probe_target = "bypass-probe";
    let workspace = build_workspace(probe_target);

    let (status, _body) = patch(workspace, probe_target).await;
    assert_eq!(status, StatusCode::OK, "opted-in write should succeed");

    let lines = captured_warnings().lock().expect("capture mutex poisoned");
    let hit = lines.iter().find(|l| l.contains(probe_target));
    let line = hit.unwrap_or_else(|| {
        panic!("expected a WARN event naming target '{probe_target}'; captured: {lines:?}")
    });
    assert!(line.contains("Message"), "expected model name in: {line}");
    assert!(line.contains("@@emit"), "expected @@emit(...) in: {line}");
}

/// A model with neither `@version` nor `@@emit` never goes through the
/// bypass branch, opt-in or not — it must not generate log noise every
/// write, only the writes the opt-in actually changes anything for.
#[tokio::test]
async fn unaffected_model_write_on_an_opted_in_target_logs_nothing() {
    init_tracing();
    let probe_target = "unaffected-probe";
    let schema = Arc::new(cratestack_parser::parse_schema(SCHEMA).expect("schema parses"));
    let target = LoadedTarget {
        key: probe_target.to_owned(),
        display_name: probe_target.to_owned(),
        mode: TargetMode::Rw,
        schema: schema.clone(),
        schema_path: PathBuf::from("schema.cstack"),
        source: Arc::new(SqliteSource::new(seeded_conn(), schema)),
        has_db: true,
        has_api: false,
        allow_unsafe_db_writes: true,
    };
    let workspace = Arc::new(LoadedWorkspace {
        config: WorkspaceConfig {
            name: "unsafe-write-signals".to_owned(),
            default_mode: TargetMode::Ro,
            cors_dev: true,
            audit_file: None,
        },
        targets: vec![Arc::new(target)],
        audit: Arc::new(AuditLog::new()),
    });

    let app = cratestack_studio::server::build_router(workspace);
    let response = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!(
                    "/api/targets/{probe_target}/models/Plain/records/p1"
                ))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({ "name": "updated" })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::OK);

    let lines = captured_warnings().lock().expect("capture mutex poisoned");
    assert!(
        lines.iter().all(|l| !l.contains(probe_target)),
        "an unaffected model must not log an unsafe-write warning: {lines:?}"
    );
}

/// `GET /api/audit` (the same endpoint the Studio UI polls) must be able
/// to tell a bypass write apart from an ordinary one.
#[tokio::test]
async fn audit_endpoint_marks_a_bypass_write_as_unsafe() {
    let probe_target = "audit_probe_bypass_507";
    let workspace = build_workspace(probe_target);

    let (status, _) = patch(workspace.clone(), probe_target).await;
    assert_eq!(status, StatusCode::OK);

    let app = cratestack_studio::server::build_router(workspace);
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/audit")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body");
    let body: Value = serde_json::from_slice(&bytes).expect("audit response is json");
    let entries = body["entries"].as_array().expect("entries array");
    let entry = entries
        .iter()
        .find(|e| e["target"] == probe_target)
        .expect("audit entry for this target present");
    assert_eq!(
        entry["unsafe_write"], true,
        "a write allowed only via allow_unsafe_writes must be marked unsafe_write:true \
         in the audit trail: {entry}"
    );
}
