//! The decisive coverage for cratestack#507 "option 3": a Studio
//! `[target.db]` `PATCH` against a live Postgres target must (1) bump
//! the model's `@version` column for real and (2) write exactly one
//! `cratestack_event_outbox` row for the operation — using the
//! reproduction shape from the issue itself (a `Message` model with
//! `version Int @version` and `@@emit(created, updated, deleted)`,
//! edited with no `If-Match` header, matching the issue's own `curl`).
//!
//! The regression test that matters most
//! (`stale_conditional_update_no_longer_succeeds_after_studio_write`)
//! proves the actual harm the issue reported is closed: a caller
//! holding the pre-edit version can no longer overwrite the row
//! believing nothing changed, because the version really did move.
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
use sqlx_core::pool::PoolOptions;
use sqlx_postgres::{PgPool, Postgres};
use testcontainers::ContainerAsync;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres as PostgresImage;
use tower::ServiceExt;

const SCHEMA: &str = r#"
model Message {
  id String @id
  version Int @version
  stateReason String
  @@emit(created, updated, deleted)
}
"#;

const TABLE: &str = "messages";

async fn serial_guard() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await
}

/// A live PG connection, plus (when the testcontainers backend is in
/// use) the container guard — dropping it stops and removes the
/// container, so a test that holds this for its whole body gets
/// automatic cleanup. Mirrors `cratestack-pg/tests/support/pg.rs` /
/// `cratestack-outbox/tests/support/pg.rs`'s `TestPg`; this crate has no
/// shared test-support module of its own to import that from yet, so
/// it's duplicated here rather than introducing one for a single file.
struct TestPg {
    pool: PgPool,
    _container: Option<ContainerAsync<PostgresImage>>,
}

/// Backend priority: `CRATESTACK_TEST_DATABASE_URL` (external PG, e.g.
/// `just pg-up`) first, then `CRATESTACK_USE_TESTCONTAINERS=1` (ephemeral
/// per-binary container), then skip. `CRATESTACK_REQUIRE_DB` turns a
/// connection failure — including neither variable being set — into a
/// panic instead of a skip, so a broken Docker (or a run that forgot to
/// set either variable) can't silently green this decisive test.
async fn connect_or_skip() -> Option<TestPg> {
    let require = std::env::var("CRATESTACK_REQUIRE_DB").is_ok();

    fn need<T, E: std::fmt::Display>(r: Result<T, E>, require: bool, ctx: &str) -> Option<T> {
        match r {
            Ok(v) => Some(v),
            Err(e) if require => panic!("CRATESTACK_REQUIRE_DB is set but {ctx} failed: {e}"),
            Err(_) => None,
        }
    }

    if let Ok(url) = std::env::var("CRATESTACK_TEST_DATABASE_URL") {
        let pool = need(
            PoolOptions::<Postgres>::new()
                .max_connections(2)
                .connect(&url)
                .await,
            require,
            "connecting to CRATESTACK_TEST_DATABASE_URL",
        )?;
        return Some(TestPg {
            pool,
            _container: None,
        });
    }

    if std::env::var("CRATESTACK_USE_TESTCONTAINERS").is_ok() {
        let container = need(
            PostgresImage::default().start().await,
            require,
            "starting the Postgres testcontainer (is Docker available?)",
        )?;
        let host = need(
            container.get_host().await,
            require,
            "resolving testcontainer host",
        )?;
        let port = need(
            container.get_host_port_ipv4(5432).await,
            require,
            "resolving testcontainer port",
        )?;
        let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
        let pool = need(
            PoolOptions::<Postgres>::new()
                .max_connections(2)
                .connect(&url)
                .await,
            require,
            "connecting to the Postgres testcontainer",
        )?;
        return Some(TestPg {
            pool,
            _container: Some(container),
        });
    }

    if require {
        panic!(
            "CRATESTACK_REQUIRE_DB is set but neither CRATESTACK_TEST_DATABASE_URL nor \
             CRATESTACK_USE_TESTCONTAINERS is — this decisive test must run against a real \
             Postgres, not skip silently"
        );
    }
    eprintln!(
        "skipping: neither CRATESTACK_TEST_DATABASE_URL nor CRATESTACK_USE_TESTCONTAINERS is set"
    );
    None
}

/// Baseline straight from SQL, mirroring the issue's own reproduction:
/// `version=0`, zero outbox rows, no Studio involvement yet.
///
/// `cratestack_event_outbox` is created (empty) here rather than left
/// for the routed write to bootstrap lazily — a real deployment would
/// already have it from the generated server's own prior writes, and
/// creating it upfront lets the "zero outbox rows" baseline assertion
/// query the table instead of tripping over "relation does not exist".
async fn seed(pool: &PgPool) {
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
        format!("INSERT INTO \"{TABLE}\" VALUES ('msgverify01', 0, 'accepted')"),
    ] {
        sqlx_core::query::query(&sql)
            .execute(pool)
            .await
            .expect("fixture ddl");
    }
    cratestack_sqlx::ensure_event_outbox_table(pool)
        .await
        .expect("outbox table bootstrap");
}

fn build_workspace(pool: &PgPool) -> Arc<LoadedWorkspace> {
    let schema = Arc::new(cratestack_parser::parse_schema(SCHEMA).expect("schema parses"));
    let target = LoadedTarget {
        key: "vsms".to_owned(),
        display_name: "vsms".to_owned(),
        mode: TargetMode::Rw,
        schema: schema.clone(),
        schema_path: PathBuf::from("schema.cstack"),
        source: Arc::new(PostgresSource::new(pool.clone(), schema)),
        has_db: true,
        has_api: false,
        allow_unsafe_db_writes: false,
    };
    Arc::new(LoadedWorkspace {
        config: WorkspaceConfig {
            name: "verify".to_owned(),
            default_mode: TargetMode::Ro,
            cors_dev: true,
            audit_file: None,
        },
        targets: vec![Arc::new(target)],
        audit: Arc::new(AuditLog::new()),
    })
}

/// The issue's own `curl` reproduction: no `If-Match`, no auth.
async fn patch_without_if_match(workspace: Arc<LoadedWorkspace>) -> (StatusCode, Value) {
    let app = cratestack_studio::server::build_router(workspace);
    let response = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/api/targets/vsms/models/Message/records/msgverify01")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(
                        &serde_json::json!({ "stateReason": "written by studio, no If-Match" }),
                    )
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

async fn outbox_row_count(pool: &PgPool, model: &str, operation: &str) -> i64 {
    let (count,): (i64,) = sqlx_core::query_as::query_as(
        "SELECT COUNT(*) FROM cratestack_event_outbox WHERE model = $1 AND operation = $2",
    )
    .bind(model)
    .bind(operation)
    .fetch_one(pool)
    .await
    .expect("outbox table exists once the routed write has run");
    count
}

/// Steps 1-3 of the task's decisive test: a Studio PATCH through
/// `[target.db]` bumps `@version` and writes exactly one outbox row.
#[tokio::test]
async fn studio_patch_bumps_version_and_writes_one_outbox_row() {
    let Some(test_pg) = connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    let _guard = serial_guard().await;
    seed(&pool).await;
    assert_eq!(
        outbox_row_count(&pool, "Message", "updated").await,
        0,
        "baseline: no outbox row before Studio touches the row"
    );

    let (status, body) = patch_without_if_match(build_workspace(&pool)).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["row"]["stateReason"], "written by studio, no If-Match");
    assert_eq!(
        body["row"]["version"], 1,
        "@version must be bumped by the routed write: {body}"
    );
    assert_eq!(
        outbox_row_count(&pool, "Message", "updated").await,
        1,
        "exactly one cratestack_event_outbox row for the update"
    );
}

/// Step 4, the regression that matters most: a caller holding the
/// pre-edit version (`0`) can no longer win a conditional update after
/// Studio's write — proving the CAS problem the issue reported (a stale
/// `if_match` silently succeeding) is actually closed, not just that a
/// number changed.
#[tokio::test]
async fn stale_conditional_update_no_longer_succeeds_after_studio_write() {
    let Some(test_pg) = connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    let _guard = serial_guard().await;
    seed(&pool).await;

    let (status, _) = patch_without_if_match(build_workspace(&pool)).await;
    assert_eq!(status, StatusCode::OK);

    // Simulate the exact CAS check the generated server's descriptor
    // path runs (`UPDATE ... WHERE id = $1 AND version = $2`) with the
    // version a caller would have held from *before* the Studio edit.
    let stale_result = sqlx_core::query::query(
        "UPDATE \"messages\" SET state_reason = 'overwritten by stale caller' \
         WHERE id = $1 AND version = $2",
    )
    .bind("msgverify01")
    .bind(0_i32)
    .execute(&pool)
    .await
    .expect("conditional update runs");
    assert_eq!(
        stale_result.rows_affected(),
        0,
        "a stale If-Match/if_match=0 check must fail after Studio's write — \
         if this affects a row, the version wasn't really bumped and CAS \
         silently does not apply, which is the exact harm cratestack#507 reported"
    );

    let (state_reason,): (String,) =
        sqlx_core::query_as::query_as("SELECT state_reason FROM \"messages\" WHERE id = $1")
            .bind("msgverify01")
            .fetch_one(&pool)
            .await
            .expect("row still present");
    assert_eq!(
        state_reason, "written by studio, no If-Match",
        "the stale conditional update must not have landed"
    );
}
