//! What the Postgres-backed tests share: a seeded database, and one
//! `tools/call` through the HTTP app with the credential headers a test
//! chooses.

// Each test target uses only some of these; Rust checks dead code per target.
#![allow(dead_code)]

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{HeaderMap, Request, StatusCode};
use cratestack::serde_json::{Value as Json, json};
use mcp_operator_example::http::{HttpConfig, app};
use mcp_operator_example::{ensure_schema, schema};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, ImageExt};
use testcontainers_modules::postgres::Postgres;
use tower::ServiceExt;

pub const KEY: &[u8] = b"test-only signing key, at least 32 bytes long";
pub const RESOURCE: &str = "http://localhost/mcp";

/// A seeded database: `CRATESTACK_TEST_DATABASE_URL` when it is set, else a
/// Postgres container, which the test holds and dropping removes (a
/// `mem::forget` kept it running after the binary exited). `None` when
/// Docker is missing, unless `CRATESTACK_REQUIRE_DB` makes that a failure.
pub async fn db_or_skip() -> Option<(Option<ContainerAsync<Postgres>>, schema::Cratestack)> {
    let (container, url) = match std::env::var("CRATESTACK_TEST_DATABASE_URL") {
        Ok(url) => (None, url),
        Err(_) => {
            let require = std::env::var("CRATESTACK_REQUIRE_DB").is_ok();
            let container = match Postgres::default().with_tag("18-alpine").start().await {
                Ok(container) => container,
                Err(error) if require => {
                    panic!("CRATESTACK_REQUIRE_DB is set but Docker failed: {error}")
                }
                Err(_) => return None,
            };
            let port = container.get_host_port_ipv4(5432).await.expect("port");
            let host = container.get_host().await.expect("host");
            let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
            (Some(container), url)
        }
    };
    let pool = cratestack::sqlx::PgPool::connect(&url)
        .await
        .expect("connect");
    ensure_schema(&pool).await.expect("seed");
    Some((container, schema::Cratestack::builder(pool).build()))
}

/// The JSON envelope a tool result carries as text.
pub fn envelope(result: &Json) -> Json {
    cratestack::serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

/// `Authorization: Bearer <token>`.
pub fn bearer(token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
    headers
}

/// One `tools/call` through the HTTP app with `credentials`: the status,
/// the `WWW-Authenticate` challenge if any, and the JSON-RPC answer.
pub async fn http(
    db: &schema::Cratestack,
    credentials: HeaderMap,
    tool: &str,
    arguments: Json,
) -> (StatusCode, Option<String>, Json) {
    let config = HttpConfig {
        resource: RESOURCE.to_owned(),
        allowed_origins: vec!["http://localhost:6274".to_owned()],
        key: KEY.to_vec(),
    };
    let body = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": tool, "arguments": arguments, "_meta": {
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientCapabilities": {},
        }},
    });
    let mut request = Request::post("/mcp")
        .header("host", "localhost")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2026-07-28")
        .header("mcp-method", "tools/call")
        .header("mcp-name", tool)
        .body(Body::from(body.to_string()))
        .unwrap();
    request.headers_mut().extend(credentials);
    let response = app(db.clone(), &config)
        .unwrap()
        .oneshot(request)
        .await
        .unwrap();
    let status = response.status();
    let challenge = response
        .headers()
        .get("www-authenticate")
        .map(|value| value.to_str().unwrap().to_owned());
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let answer = cratestack::serde_json::from_slice(&bytes).unwrap_or(Json::Null);
    (status, challenge, answer)
}
