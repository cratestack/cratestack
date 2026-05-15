//! HTTP-level smoke tests for the read endpoints. Boots a router
//! against two targets:
//!
//! - An API-only target pointed at an unreachable host so we can
//!   verify the BAD_GATEWAY / UNSUPPORTED behavior contractually.
//! - A SQLite target backed by an in-memory database so we exercise
//!   the live read path (list, get, follow) end-to-end without
//!   needing a real Postgres.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cratestack_studio::config::{TargetMode, WorkspaceConfig};
use cratestack_studio::data::api::ApiSource;
use cratestack_studio::data::sqlite::SqliteSource;
use cratestack_studio::workspace::{LoadedTarget, LoadedWorkspace};
use serde_json::Value;
use tower::ServiceExt;

const BLOG_SCHEMA: &str = r#"
model Customer {
  id Int @id
  email String
  posts Post[] @relation(fields: [id], references: [authorId])
}

model Post {
  id String @id
  authorId Int
  title String
  author Customer @relation(fields: [authorId], references: [id])
}
"#;

fn build_workspace() -> Arc<LoadedWorkspace> {
    let schema = Arc::new(
        cratestack_parser::parse_schema(BLOG_SCHEMA).expect("schema parses"),
    );

    let api_source = ApiSource::new(
        "https://example.test".to_owned(),
        None,
        schema.clone(),
    )
    .expect("ApiSource builds");
    let api_target = LoadedTarget {
        key: "api".to_owned(),
        display_name: "Demo API".to_owned(),
        mode: TargetMode::Ro,
        schema: schema.clone(),
        schema_path: PathBuf::from("schema.cstack"),
        source: Arc::new(api_source),
        has_db: false,
        has_api: true,
    };

    let conn = rusqlite::Connection::open_in_memory().expect("sqlite open");
    conn.execute_batch(
        r#"
        CREATE TABLE customers (id INTEGER PRIMARY KEY, email TEXT NOT NULL);
        INSERT INTO customers VALUES
          (1, 'alice@example.com'),
          (2, 'bob@example.com');
        CREATE TABLE posts (
          id TEXT PRIMARY KEY,
          author_id INTEGER NOT NULL,
          title TEXT NOT NULL
        );
        INSERT INTO posts VALUES
          ('p1', 1, 'first'),
          ('p2', 1, 'second'),
          ('p3', 2, 'third');
        "#,
    )
    .expect("ddl");
    let sqlite_target = LoadedTarget {
        key: "sqlite".to_owned(),
        display_name: "Demo SQLite".to_owned(),
        mode: TargetMode::Ro,
        schema: schema.clone(),
        schema_path: PathBuf::from("schema.cstack"),
        source: Arc::new(SqliteSource::new(conn, schema)),
        has_db: true,
        has_api: false,
    };

    Arc::new(LoadedWorkspace {
        config: WorkspaceConfig {
            name: "smoke".to_owned(),
            default_mode: TargetMode::Ro,
            cors_dev: true,
        },
        targets: vec![Arc::new(api_target), Arc::new(sqlite_target)],
    })
}

async fn json_get(uri: &str) -> (StatusCode, Value) {
    let app = cratestack_studio::server::build_router(build_workspace());
    let response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
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
async fn list_targets_returns_workspace_and_capabilities() {
    let (status, body) = json_get("/api/targets").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["workspace"], "smoke");
    let keys: Vec<&str> = body["targets"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["key"].as_str())
        .collect();
    assert!(keys.contains(&"api"));
    assert!(keys.contains(&"sqlite"));
}

#[tokio::test]
async fn target_schema_returns_owned_schema_summary() {
    let (status, body) = json_get("/api/targets/sqlite/schema").await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<&str> = body["models"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m.as_str())
        .collect();
    assert!(names.contains(&"Customer"));
    assert!(names.contains(&"Post"));
}

#[tokio::test]
async fn list_models_returns_primary_keys_and_fields() {
    let (status, body) = json_get("/api/targets/sqlite/models").await;
    assert_eq!(status, StatusCode::OK);
    let post = body["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["name"] == "Post")
        .expect("Post present");
    assert_eq!(post["primary_key"], "id");
    let author_field = post["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "author")
        .unwrap();
    assert_eq!(author_field["is_relation"], true);
}

#[tokio::test]
async fn snippet_renders_owned_string_literal() {
    let (status, body) =
        json_get("/api/targets/sqlite/models/Post/snippet?pk=p1").await;
    assert_eq!(status, StatusCode::OK);
    let rust = body["rust"].as_str().expect("rust");
    assert!(rust.contains("cool.post()"), "{rust}");
    assert!(rust.contains(".find_unique(\"p1\".to_owned())"), "{rust}");
}

#[tokio::test]
async fn snippet_renders_int_literal_for_int_pk() {
    let (status, body) =
        json_get("/api/targets/sqlite/models/Customer/snippet?pk=42").await;
    assert_eq!(status, StatusCode::OK);
    let rust = body["rust"].as_str().expect("rust");
    assert!(rust.contains(".find_unique(42_i64)"), "{rust}");
}

#[tokio::test]
async fn list_records_against_sqlite_returns_rows() {
    let (status, body) = json_get(
        "/api/targets/sqlite/models/Post/records?limit=2",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let rows = body["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(body["next_cursor"], "p2");
}

#[tokio::test]
async fn get_record_against_sqlite_returns_row() {
    let (status, body) =
        json_get("/api/targets/sqlite/models/Post/records/p2").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["row"]["title"], "second");
}

#[tokio::test]
async fn follow_outgoing_returns_single_row() {
    let (status, body) = json_get(
        "/api/targets/sqlite/models/Post/records/p1/rel/author",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["row"]["email"], "alice@example.com");
}

#[tokio::test]
async fn follow_inbound_one_to_many_returns_page() {
    let (status, body) = json_get(
        "/api/targets/sqlite/models/Customer/records/1/rel/posts",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let rows = body["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let titles: Vec<&str> = rows
        .iter()
        .filter_map(|r| r["title"].as_str())
        .collect();
    assert!(titles.contains(&"first"));
    assert!(titles.contains(&"second"));
}

#[tokio::test]
async fn follow_unknown_field_returns_404() {
    let (status, body) = json_get(
        "/api/targets/sqlite/models/Post/records/p1/rel/nope",
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "UNKNOWN_FIELD");
}

#[tokio::test]
async fn follow_non_relation_field_returns_400() {
    let (status, body) = json_get(
        "/api/targets/sqlite/models/Post/records/p1/rel/title",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "NOT_A_RELATION");
}

#[tokio::test]
async fn list_records_against_api_target_returns_bad_gateway() {
    // The configured base_url is unreachable; ApiSource attempts the
    // upstream call and surfaces a 502.
    let (status, body) =
        json_get("/api/targets/api/models/Post/records").await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(body["error"]["code"], "UPSTREAM_ERROR");
}

#[tokio::test]
async fn unknown_target_returns_404() {
    let (status, body) = json_get("/api/targets/missing/schema").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "UNKNOWN_TARGET");
}

#[tokio::test]
async fn unknown_model_in_snippet_returns_404() {
    let (status, body) =
        json_get("/api/targets/sqlite/models/Nope/snippet?pk=1").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "UNKNOWN_MODEL");
}

#[tokio::test]
async fn index_page_responds_200() {
    let app = cratestack_studio::server::build_router(build_workspace());
    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn health_endpoint_reflects_workspace() {
    let (status, body) = json_get("/api/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["workspace"], "smoke");
    assert_eq!(body["target_count"], 2);
}

/// With the `embed-ui` feature on (and `trunk build` already run in
/// `ui/`), `/` returns the bundled UI's index.html — not the Phase 1b
/// stub. Without the feature, `/` returns the stub. Both branches are
/// here so the test suite covers the feature matrix.
#[cfg(feature = "embed-ui")]
#[tokio::test]
async fn root_serves_bundled_ui_index_when_feature_on() {
    let app = cratestack_studio::server::build_router(build_workspace());
    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body");
    let text = std::str::from_utf8(&bytes).expect("utf-8");
    assert!(
        text.contains("cratestack-studio"),
        "bundled index.html should contain 'cratestack-studio'; got: {}",
        &text[..text.len().min(200)]
    );
    assert!(
        text.contains("data-trunk") || text.contains(".wasm") || text.contains(".js"),
        "bundled index.html should reference trunk-injected assets"
    );
}

#[cfg(feature = "embed-ui")]
#[tokio::test]
async fn unknown_static_path_falls_back_to_index_html() {
    let app = cratestack_studio::server::build_router(build_workspace());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/some/spa/route")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::OK);
    let ct = response
        .headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap_or("").to_owned())
        .unwrap_or_default();
    assert!(ct.contains("html"), "fallback should be HTML, got {ct}");
}

#[cfg(feature = "embed-ui")]
#[tokio::test]
async fn api_route_takes_precedence_over_ui_fallback() {
    // /api/* must hit the JSON API, not the SPA fallback.
    let (status, body) = json_get("/api/targets").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["workspace"], "smoke");
}

#[cfg(not(feature = "embed-ui"))]
#[tokio::test]
async fn root_serves_stub_page_when_feature_off() {
    let app = cratestack_studio::server::build_router(build_workspace());
    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body");
    let text = std::str::from_utf8(&bytes).expect("utf-8");
    assert!(
        text.contains("Phase 1a backend") || text.contains("Phase 1b backend"),
        "stub page should describe the current phase: {}",
        &text[..text.len().min(200)]
    );
}

#[tokio::test]
async fn cors_headers_present_on_api_responses() {
    let app = cratestack_studio::server::build_router(build_workspace());
    let response = app
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/api/targets")
                .header("Origin", "http://localhost:8080")
                .header("Access-Control-Request-Method", "GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request");
    let allow_origin = response
        .headers()
        .get("access-control-allow-origin")
        .map(|v| v.to_str().unwrap_or(""));
    assert!(allow_origin.is_some(), "CORS allow-origin should be set");
}
