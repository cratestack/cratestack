//! A `@server_only` field never reaches an MCP `tools/call` result, on a
//! model that also has a `@computed` field. Tool dispatch composes a
//! computed-bearing output through the same `compose_<owner>_value`
//! helpers REST does (ADR 0002 Q7), so it inherited their leak of
//! `@server_only` fields; `tests/server_only_outbound.rs` is the REST and
//! RPC side and says more.
//!
//! Every tool here returns widgets whose `secret` is set, as a row loaded
//! from the database would have it, so no database is needed (`connect_lazy`).
//! Gated `required-features = ["mcp"]`; `just test-ci-host` runs it.

#[macro_use]
mod server_only_outbound_support;

use cratestack::mcp::StdioServer;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{CratestackContext, Value, include_server_schema};
use serde_json::{Value as Json, json};
use server_only_outbound_support::{LABEL, assert_no_server_only, procedure_cases};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

include_server_schema!(
    "tests/fixtures/server_only_outbound_mcp.cstack",
    db = Postgres
);
so_out_impls!();

/// One `tools/call` over stdio framing: the raw response line, as an MCP
/// client receives it.
async fn call(name: &str) -> String {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    let db = cratestack_schema::Cratestack::builder(pool).build();
    let tools = cratestack_schema::mcp::tools(db, Procedures, Resolvers);
    let ctx = CratestackContext::authenticated([("id".to_owned(), Value::Int(1))]);
    let server = StdioServer::new(tools, ctx).expect("generated table");
    let (mut writer, server_reader) = tokio::io::duplex(1 << 16);
    let (server_writer, reader) = tokio::io::duplex(1 << 16);
    tokio::spawn(server.serve_io(server_reader, server_writer));
    let request = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {
            "name": name, "arguments": { "label": LABEL },
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {},
            },
        },
    });
    writer
        .write_all(format!("{request}\n").as_bytes())
        .await
        .unwrap();
    BufReader::new(reader)
        .lines()
        .next_line()
        .await
        .unwrap()
        .expect("the server answered")
}

/// Both copies of the output are checked: `structuredContent` (sent when
/// the output is an object) by the leak check over the whole line, and the
/// text block by exact comparison.
#[tokio::test]
async fn a_tool_result_never_carries_a_server_only_field() {
    for (name, pointer, expected) in procedure_cases() {
        let line = call(name).await;
        assert_no_server_only(name, line.as_bytes());
        let response: Json = serde_json::from_str(&line).expect("JSON-RPC response");
        let result = &response["result"];
        assert_eq!(result["isError"], json!(false), "{name}: {line}");
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{name}: no text block: {line}"));
        let output: Json = serde_json::from_str(text).expect("the text block is JSON");
        assert_no_server_only(name, text.as_bytes());
        let compared = output
            .pointer(pointer)
            .unwrap_or_else(|| panic!("{name}: no `{pointer}` in {output}"));
        assert_eq!(compared, &expected, "{name}");
        if let Some(structured) = result.get("structuredContent") {
            assert_eq!(structured, &output, "{name}: the two copies differ");
        }
    }
}
