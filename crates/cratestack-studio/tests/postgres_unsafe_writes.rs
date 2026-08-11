//! Live-Postgres confirmation that cratestack#507's write guard applies
//! identically to a `PostgresSource`-backed target, not just the
//! `SqliteSource` fixtures in `tests/unsafe_db_writes.rs`.
//!
//! The guard itself (`require_safe_write`) runs in the HTTP handler
//! before any `DataSource` method is called, so it is backend-agnostic
//! by construction — this file exists to prove that in practice rather
//! than only by inspection, the same way `postgres_row_keys.rs` proves
//! a Postgres-specific behavior a SQL-string test can't.
//!
//! Skips silently unless `CRATESTACK_TEST_DATABASE_URL` is set — the
//! same convention every other PG-backed test in the workspace uses.
//! `just test-pg` sets it.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cratestack_studio::audit::AuditLog;
use cratestack_studio::config::{TargetMode, WorkspaceConfig};
use cratestack_studio::data::postgres::PostgresSource;
use cratestack_studio::workspace::{LoadedTarget, LoadedWorkspace};
use serde_json::Value;
use sqlx_core::pool::PoolOptions;
use sqlx_postgres::{PgPool, Postgres};
use tower::ServiceExt;

const SCHEMA: &str = r#"
model StudioUnsafeWriteProbe {
  id String @id
  version Int @version
  stateReason String
  @@emit(created, updated, deleted)
}
"#;

const TABLE: &str = "studio_unsafe_write_probes";

async fn serial_guard() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await
}

async fn connect_or_skip() -> Option<PgPool> {
    let url = std::env::var("CRATESTACK_TEST_DATABASE_URL").ok()?;
    PoolOptions::<Postgres>::new()
        .max_connections(2)
        .connect(&url)
        .await
        .ok()
}

async fn build_workspace(pool: &PgPool, allow_unsafe_db_writes: bool) -> Arc<LoadedWorkspace> {
    for sql in [
        format!("DROP TABLE IF EXISTS \"{TABLE}\""),
        format!(
            "CREATE TABLE \"{TABLE}\" (
               id TEXT PRIMARY KEY,
               version INTEGER NOT NULL,
               state_reason TEXT NOT NULL
             )"
        ),
        format!("INSERT INTO \"{TABLE}\" VALUES ('msg1', 0, 'accepted')"),
    ] {
        sqlx_core::query::query(&sql)
            .execute(pool)
            .await
            .expect("fixture ddl");
    }
    let schema = Arc::new(cratestack_parser::parse_schema(SCHEMA).expect("schema parses"));
    let target = LoadedTarget {
        key: "pg".to_owned(),
        display_name: "pg".to_owned(),
        mode: TargetMode::Rw,
        schema: schema.clone(),
        schema_path: PathBuf::from("schema.cstack"),
        source: Arc::new(PostgresSource::new(pool.clone(), schema)),
        has_db: true,
        has_api: false,
        allow_unsafe_db_writes,
    };
    Arc::new(LoadedWorkspace {
        config: WorkspaceConfig {
            name: "pg-unsafe-writes".to_owned(),
            default_mode: TargetMode::Ro,
            cors_dev: true,
            audit_file: None,
        },
        targets: vec![Arc::new(target)],
        audit: Arc::new(AuditLog::new()),
    })
}

async fn patch(workspace: Arc<LoadedWorkspace>) -> (StatusCode, Value) {
    let app = cratestack_studio::server::build_router(workspace);
    let response = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/api/targets/pg/models/StudioUnsafeWriteProbe/records/msg1")
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
async fn postgres_target_refuses_versioned_emitting_write_without_opt_in() {
    let Some(pool) = connect_or_skip().await else {
        eprintln!("skipping: CRATESTACK_TEST_DATABASE_URL unset");
        return;
    };
    let _guard = serial_guard().await;
    let workspace = build_workspace(&pool, false).await;

    let (status, body) = patch(workspace).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "UNSAFE_DB_WRITE");

    // The refusal happened before any SQL ran — the row is untouched.
    let row: (String,) = sqlx_core::query_as::query_as(&format!(
        "SELECT state_reason FROM \"{TABLE}\" WHERE id = 'msg1'"
    ))
    .fetch_one(&pool)
    .await
    .expect("row still present");
    assert_eq!(row.0, "accepted");
}

#[tokio::test]
async fn postgres_target_allows_versioned_emitting_write_with_opt_in() {
    let Some(pool) = connect_or_skip().await else {
        eprintln!("skipping: CRATESTACK_TEST_DATABASE_URL unset");
        return;
    };
    let _guard = serial_guard().await;
    let workspace = build_workspace(&pool, true).await;

    let (status, body) = patch(workspace).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["row"]["stateReason"], "written by studio");
    // Bypass confirmed real even when chosen: version is untouched.
    assert_eq!(body["row"]["version"], 0);
}
