//! The example's own wiring against real Postgres, without Node: the same
//! table served over stdio framing and over the HTTP app `main.rs` runs.
//! `just mcp-conformance` is the third-party-client run; this is what
//! `cargo test` alone can prove about the code in `src/`.
//!
//! Skips (prints `ok` having exercised nothing) when Docker is missing. Set
//! `CRATESTACK_REQUIRE_DB=1` to make that a failure, and read `finished in`:
//! a real run takes seconds, a skip `0.00s`.

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::mcp::StdioServer;
use cratestack::serde_json::{Value as Json, json};
use mcp_operator_example::http::{HttpConfig, app};
use mcp_operator_example::token::{STDIO_AUDIENCE, TokenVerifier, mint};
use mcp_operator_example::{ensure_schema, mcp_table, schema};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, ImageExt};
use testcontainers_modules::postgres::Postgres;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tower::ServiceExt;

const KEY: &[u8] = b"test-only signing key, at least 32 bytes long";
const RESOURCE: &str = "http://localhost/mcp";

/// The container comes back with the database: the test holds it, and
/// dropping it at the end removes it. `mem::forget` kept it running after
/// the test binary exited, one leaked Postgres per local run.
async fn db_or_skip() -> Option<(ContainerAsync<Postgres>, schema::Cratestack)> {
    let require = std::env::var("CRATESTACK_REQUIRE_DB").is_ok();
    let container = match Postgres::default().with_tag("18-alpine").start().await {
        Ok(container) => container,
        Err(error) if require => panic!("CRATESTACK_REQUIRE_DB is set but Docker failed: {error}"),
        Err(_) => return None,
    };
    let port = container.get_host_port_ipv4(5432).await.expect("port");
    let host = container.get_host().await.expect("host");
    let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
    let pool = cratestack::sqlx::PgPool::connect(&url)
        .await
        .expect("connect");
    ensure_schema(&pool).await.expect("seed");
    Some((container, schema::Cratestack::builder(pool).build()))
}

/// One request over stdio framing, as `(id, role)`; the JSON-RPC answer.
async fn stdio(db: &schema::Cratestack, role: &str, method: &str, params: Json) -> Json {
    let token = mint(KEY, STDIO_AUDIENCE, "u-1", role, 60);
    let ctx = TokenVerifier::new(KEY, STDIO_AUDIENCE)
        .unwrap()
        .verify(&token)
        .unwrap();
    let server = StdioServer::new(mcp_table(db.clone()), ctx).expect("table");
    let (mut writer, server_reader) = tokio::io::duplex(1 << 16);
    let (server_writer, reader) = tokio::io::duplex(1 << 16);
    tokio::spawn(server.serve_io(server_reader, server_writer));
    let mut params = params;
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
    });
    let request = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
    writer
        .write_all(format!("{request}\n").as_bytes())
        .await
        .unwrap();
    let line = BufReader::new(reader).lines().next_line().await.unwrap();
    cratestack::serde_json::from_str(&line.expect("an answer")).unwrap()
}

fn envelope(result: &Json) -> Json {
    cratestack::serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

/// One `tools/call` through the HTTP app with a bearer token; the status
/// and the JSON-RPC answer.
async fn http(
    db: &schema::Cratestack,
    token: &str,
    tool: &str,
    arguments: Json,
) -> (StatusCode, Json) {
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
    let request = Request::post("/mcp")
        .header("host", "localhost")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2026-07-28")
        .header("mcp-method", "tools/call")
        .header("mcp-name", tool)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app(db.clone(), &config)
        .unwrap()
        .oneshot(request)
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        cratestack::serde_json::from_slice(&bytes).unwrap_or(Json::Null),
    )
}

#[tokio::test]
async fn the_example_serves_both_transports_under_its_policies() {
    let Some((_container, db)) = db_or_skip().await else {
        return;
    };

    // stdio: a member's publish is refused by the procedure's `@allow`.
    let denied = stdio(
        &db,
        "member",
        "tools/call",
        json!({ "name": "publish_post", "arguments": { "id": 3 } }),
    )
    .await;
    assert_eq!(denied["result"]["isError"], json!(true), "{denied}");
    assert_eq!(
        envelope(&denied["result"])["message"],
        "procedure policy denied this operation"
    );

    // An editor's runs, and returns the updated row.
    let published = stdio(
        &db,
        "editor",
        "tools/call",
        json!({ "name": "publish_post", "arguments": { "id": 2 } }),
    )
    .await;
    assert_eq!(
        published["result"]["structuredContent"]["published"],
        json!(true),
        "{published}"
    );

    // u-2's draft is hidden from u-1, exactly like a row that does not exist.
    let hidden = stdio(
        &db,
        "member",
        "resources/read",
        json!({ "uri": "cratestack://blog/posts/3" }),
    )
    .await;
    let missing = stdio(
        &db,
        "member",
        "resources/read",
        json!({ "uri": "cratestack://blog/posts/999" }),
    )
    .await;
    assert!(hidden["result"].is_null(), "{hidden}");
    assert_eq!(hidden["error"], missing["error"]);

    // HTTP: the provider's context reaches the policy check.
    let member = mint(KEY, RESOURCE, "u-1", "member", 60);
    let (status, answer) = http(&db, &member, "recent_posts", json!({ "limit": 10 })).await;
    assert_eq!(status, StatusCode::OK);
    let rows = envelope(&answer["result"]);
    let ids: Vec<i64> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids, [4, 2, 1], "newest first, u-2's draft absent");

    // A token minted for the stdio audience does not open the HTTP endpoint.
    let foreign = mint(KEY, STDIO_AUDIENCE, "u-1", "editor", 60);
    let (status, _) = http(&db, &foreign, "recent_posts", json!({ "limit": 10 })).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
