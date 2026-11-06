//! The example's own wiring against real Postgres, without Node: the same
//! table served over stdio framing and over the HTTP app `main.rs` runs.
//! `just mcp-conformance` is the third-party-client run; this is what
//! `cargo test` alone can prove about the code in `src/`.
//!
//! Runs against `CRATESTACK_TEST_DATABASE_URL` when set, else a Postgres
//! container. Skips (prints `ok` having exercised nothing) when Docker is
//! missing. Set `CRATESTACK_REQUIRE_DB=1` to make that a failure, and read
//! `finished in`: a real run takes seconds, a skip `0.00s`.

mod support;

use cratestack::axum::http::StatusCode;
use cratestack::mcp::StdioServer;
use cratestack::serde_json::{Value as Json, json};
use mcp_operator_example::token::{STDIO_AUDIENCE, TokenVerifier, mint};
use mcp_operator_example::{mcp_table, schema};
use support::{KEY, RESOURCE, bearer, db_or_skip, envelope, http};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

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
    let (status, _, answer) =
        http(&db, bearer(&member), "recent_posts", json!({ "limit": 10 })).await;
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
    let (status, _, _) = http(
        &db,
        bearer(&foreign),
        "recent_posts",
        json!({ "limit": 10 }),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
