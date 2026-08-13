//! Live-Postgres confirmation that cratestack#507's *refusal* path
//! (`403 UNSAFE_DB_WRITE`, introduced by PR #516) no longer fires for a
//! `PostgresSource`-backed target at all — "option 3" (routing writes
//! through the same `cratestack_event_outbox` primitives the generated
//! server uses; see `crate::api::records::guards::WriteMode` and
//! `crate::data::postgres::ops_routed`) makes both `@version` bumping
//! and `@@emit` routable on Postgres, so there is no longer a model
//! shape on this backend the refusal needs to protect.
//!
//! `tests/postgres_routed_writes.rs` is the decisive coverage for what
//! the routed write actually *does* (bumps `@version` for real, writes
//! exactly one `cratestack_event_outbox` row, and the version bump is
//! what makes a stale `if_match` correctly fail afterward). This file
//! exists only to pin down the negative: unlike `SqliteSource`
//! (`tests/unsafe_db_writes.rs`, where `@@emit` genuinely can't be
//! routed and the refusal still applies), a Postgres target with
//! `allow_unsafe_writes` left at its default (`false`) succeeds rather
//! than being refused.
//!
//! Skips silently unless `CRATESTACK_TEST_DATABASE_URL` (external PG,
//! `just test-pg`) or `CRATESTACK_USE_TESTCONTAINERS=1` (ephemeral
//! per-binary container) is set. `CRATESTACK_REQUIRE_DB=1` turns the
//! skip into a panic so a run can't go green without having run this
//! for real.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cratestack_studio::audit::AuditLog;
use cratestack_studio::config::{TargetMode, WorkspaceConfig};
use cratestack_studio::data::postgres::PostgresSource;
use cratestack_studio::workspace::{LoadedTarget, LoadedWorkspace};
use serde_json::Value;
use sqlx_postgres::PgPool;
use tower::ServiceExt;

mod support;

use support::pg;

const SCHEMA: &str = r#"
model StudioUnsafeWriteProbe {
  id String @id
  version Int @version
  stateReason String
  @@emit(created, updated, deleted)
}
"#;

const TABLE: &str = "studio_unsafe_write_probes";

async fn build_workspace(pool: &PgPool, allow_unsafe_db_writes: bool) -> Arc<LoadedWorkspace> {
    for sql in [
        format!("DROP TABLE IF EXISTS \"{TABLE}\""),
        "DROP TABLE IF EXISTS cratestack_event_outbox".to_owned(),
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
async fn postgres_target_no_longer_refuses_versioned_emitting_writes() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    let _guard = pg::serial_guard().await;
    // `allow_unsafe_writes` left at its default (`false`) on purpose:
    // the whole point is that Postgres no longer needs the opt-in for
    // this model shape.
    let workspace = build_workspace(pool, false).await;

    let (status, body) = patch(workspace).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a Postgres target routes @version/@@emit for real; it must not \
         be refused with UNSAFE_DB_WRITE: {body}"
    );
    assert_eq!(body["row"]["stateReason"], "written by studio");
    assert_eq!(
        body["row"]["version"], 1,
        "the routed write bumps @version for real: {body}"
    );
}

#[tokio::test]
async fn postgres_target_with_opt_in_still_routes_for_real_not_a_bypass() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    let _guard = pg::serial_guard().await;
    // `allow_unsafe_writes = true` here shouldn't change anything on
    // Postgres — there's nothing left to bypass on this backend.
    let workspace = build_workspace(pool, true).await;

    let (status, body) = patch(workspace).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["row"]["version"], 1,
        "opting into allow_unsafe_writes must not disable the routed \
         (real) @version bump on a backend that can route it: {body}"
    );
}
