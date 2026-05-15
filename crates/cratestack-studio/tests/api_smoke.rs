//! HTTP-level smoke tests for the read endpoints. Boots a router
//! against an API-only target so we exercise routing, JSON shapes,
//! and 404 handling without needing a live database.
//!
//! Endpoints that hit the DataSource on an API target intentionally
//! return 501 Not Implemented in Phase 1a (Phase 1b wires `list` /
//! `get` to the upstream service); the test asserts on that contract.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cratestack_studio::data::api::ApiSource;
use cratestack_studio::workspace::{LoadedTarget, LoadedWorkspace};
use cratestack_studio::config::{TargetMode, WorkspaceConfig};
use serde_json::Value;
use tower::ServiceExt;

const SCHEMA_TEXT: &str = r#"
model Post {
  id String @id
  title String
  body String?
}

model Customer {
  id Int @id
  email String
}
"#;

fn build_workspace() -> Arc<LoadedWorkspace> {
    let schema = Arc::new(
        cratestack_parser::parse_schema(SCHEMA_TEXT).expect("schema parses"),
    );

    let api_source =
        ApiSource::new("https://example.test".to_owned(), None, schema.clone())
            .expect("ApiSource builds");

    let target = LoadedTarget {
        key: "demo".to_owned(),
        display_name: "Demo".to_owned(),
        mode: TargetMode::Ro,
        schema,
        schema_path: PathBuf::from("schema.cstack"),
        source: Arc::new(api_source),
        has_db: false,
        has_api: true,
    };

    Arc::new(LoadedWorkspace {
        config: WorkspaceConfig {
            name: "smoke".to_owned(),
            default_mode: TargetMode::Ro,
        },
        targets: vec![Arc::new(target)],
    })
}

async fn json_get(uri: &str) -> (StatusCode, Value) {
    let app = cratestack_studio::server::build_router(build_workspace());
    let response = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
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
async fn list_targets_returns_workspace_and_capabilities() {
    let (status, body) = json_get("/api/targets").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["workspace"], "smoke");
    assert_eq!(body["targets"][0]["key"], "demo");
    assert_eq!(body["targets"][0]["display_name"], "Demo");
    assert_eq!(body["targets"][0]["mode"], "ro");
    assert_eq!(body["targets"][0]["has_db"], false);
    assert_eq!(body["targets"][0]["has_api"], true);
}

#[tokio::test]
async fn target_schema_returns_owned_schema_summary() {
    let (status, body) = json_get("/api/targets/demo/schema").await;
    assert_eq!(status, StatusCode::OK);
    let models = body["models"].as_array().expect("models array");
    assert_eq!(models.len(), 2);
    let names: Vec<&str> = models.iter().filter_map(|m| m.as_str()).collect();
    assert!(names.contains(&"Post"));
    assert!(names.contains(&"Customer"));
}

#[tokio::test]
async fn list_models_returns_primary_keys_and_fields() {
    let (status, body) = json_get("/api/targets/demo/models").await;
    assert_eq!(status, StatusCode::OK);
    let models = body["models"].as_array().expect("models array");
    let post = models
        .iter()
        .find(|m| m["name"] == "Post")
        .expect("Post present");
    assert_eq!(post["primary_key"], "id");
    let id_field = post["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "id")
        .unwrap();
    assert_eq!(id_field["type_name"], "String");
    assert_eq!(id_field["arity"], "required");
    assert_eq!(id_field["is_id"], true);
}

#[tokio::test]
async fn snippet_renders_owned_string_literal() {
    let (status, body) =
        json_get("/api/targets/demo/models/Post/snippet?pk=abc-123").await;
    assert_eq!(status, StatusCode::OK);
    let rust = body["rust"].as_str().expect("rust field");
    assert!(rust.contains("cool.post()"), "{rust}");
    assert!(
        rust.contains(".find_unique(\"abc-123\".to_owned())"),
        "{rust}"
    );
}

#[tokio::test]
async fn snippet_renders_int_literal_for_int_pk() {
    let (status, body) =
        json_get("/api/targets/demo/models/Customer/snippet?pk=42").await;
    assert_eq!(status, StatusCode::OK);
    let rust = body["rust"].as_str().expect("rust field");
    assert!(rust.contains(".find_unique(42_i64)"), "{rust}");
}

#[tokio::test]
async fn list_records_against_api_target_returns_501() {
    let (status, body) =
        json_get("/api/targets/demo/models/Post/records").await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(body["error"]["code"], "UNSUPPORTED");
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
        json_get("/api/targets/demo/models/Nope/snippet?pk=1").await;
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
    assert_eq!(body["target_count"], 1);
}
