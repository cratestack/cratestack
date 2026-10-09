//! ADR 0019: an MCP resource whose `@id` is a `BigInt`. A record is
//! addressed by the canonical decimal string (`cratestack://ledger/notes/
//! 9007199254740993`), read at the three pinned values, and comes back with
//! its `id` as that same string. Every spelling that is not canonical, and
//! every value outside `i64`, answers exactly like a missing row.
//!
//! Gated on the `mcp` feature by the `cfg` below. Run with a database and
//! `CRATESTACK_REQUIRE_DB=1`, or a missing database is a silent skip that
//! still prints `ok`; read `finished in` to tell.

#![cfg(feature = "mcp")]

mod support;

use cratestack::include_server_schema;
use cratestack::mcp::StdioServer;
use cratestack::{CratestackContext, Value};
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines};

use cratestack_schema::Cratestack;

include_server_schema!(
    "tests/fixtures/mcp_resources_bigint/schema.cstack",
    db = Postgres
);

#[derive(Clone)]
struct NoProcedures;

impl cratestack_schema::procedures::ProcedureRegistry for NoProcedures {}

/// ADR 0019's three pinned values, plus the ones around zero.
const IDS: [i64; 5] = [i64::MIN, 0, 7, 9_007_199_254_740_993, i64::MAX];

struct Mcp {
    writer: DuplexStream,
    lines: Lines<BufReader<DuplexStream>>,
    next_id: u64,
}

impl Mcp {
    fn start(db: &Cratestack) -> Self {
        let tools = cratestack_schema::mcp::tools(db.clone(), NoProcedures, ());
        let ctx =
            CratestackContext::authenticated([("id".to_owned(), Value::String("u-1".to_owned()))]);
        let server = StdioServer::new(tools, ctx).expect("generated table");
        let (writer, server_reader) = tokio::io::duplex(1 << 20);
        let (server_writer, reader) = tokio::io::duplex(1 << 20);
        tokio::spawn(server.serve_io(server_reader, server_writer));
        Self {
            writer,
            lines: BufReader::new(reader).lines(),
            next_id: 1,
        }
    }

    async fn read(&mut self, uri: &str) -> serde_json::Value {
        let params = json!({
            "uri": uri,
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {},
            },
        });
        let request = json!({
            "jsonrpc": "2.0", "id": self.next_id, "method": "resources/read", "params": params,
        });
        self.next_id += 1;
        self.writer
            .write_all(format!("{request}\n").as_bytes())
            .await
            .unwrap();
        let line = self.lines.next_line().await.unwrap().expect("an answer");
        serde_json::from_str(&line).unwrap()
    }
}

fn document(response: &serde_json::Value) -> serde_json::Value {
    let text = response["result"]["contents"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("not a read result: {response}"));
    serde_json::from_str(text).unwrap()
}

async fn seeded() -> Option<(support::pg::TestPg, Cratestack)> {
    let test_pg = support::pg::connect_or_skip().await?;
    for statement in [
        "DROP TABLE IF EXISTS mcp_big_notes",
        "CREATE TABLE mcp_big_notes (id BIGINT PRIMARY KEY, label TEXT NOT NULL)",
    ] {
        cratestack::sqlx::query(statement)
            .execute(&test_pg.pool)
            .await
            .expect(statement);
    }
    for id in IDS {
        cratestack::sqlx::query("INSERT INTO mcp_big_notes (id, label) VALUES ($1, $2)")
            .bind(id)
            .bind(format!("note {id}"))
            .execute(&test_pg.pool)
            .await
            .expect("seed");
    }
    let db = Cratestack::builder(test_pg.pool.clone()).build();
    Some((test_pg, db))
}

#[tokio::test]
async fn a_big_int_keyed_resource_is_read_by_its_canonical_string() {
    support::tracing_capture::init_tracing();
    let _guard = support::pg::serial_guard().await;
    let Some((_pg, db)) = seeded().await else {
        return;
    };
    let mut mcp = Mcp::start(&db);

    // Each pinned value resolves, exactly: nothing rounded `2^53 + 1`, and
    // `id` comes back as the canonical string, not a JSON number.
    for id in IDS {
        let response = mcp.read(&format!("cratestack://ledger/notes/{id}")).await;
        let record = document(&response);
        assert_eq!(record["id"], json!(id.to_string()), "{response}");
        assert_eq!(record["label"], json!(format!("note {id}")), "{response}");
    }

    // A neighbour that is not seeded is missing, and every non-canonical
    // spelling or out-of-range value answers byte-for-byte like it.
    let missing = mcp.read("cratestack://ledger/notes/8").await;
    assert_eq!(missing["error"]["message"], "resource not found");
    for tail in [
        "+7",
        "007",
        "-0",
        "7.0",
        "7e0",
        "0x7",
        "%207",
        "7%20",
        "9223372036854775808",
        "-9223372036854775809",
        "99999999999999999999",
        "",
    ] {
        let response = mcp.read(&format!("cratestack://ledger/notes/{tail}")).await;
        assert_eq!(response["error"], missing["error"], "{tail:?}: {response}");
    }

    // The collection lists every seeded row in key order, ids as strings.
    let page = document(&mcp.read("cratestack://ledger/notes").await);
    let ids: Vec<&str> = page["items"]
        .as_array()
        .unwrap_or_else(|| panic!("not a page: {page}"))
        .iter()
        .map(|item| item["id"].as_str().expect("a string id"))
        .collect();
    let expected: Vec<String> = IDS.iter().map(i64::to_string).collect();
    assert_eq!(ids, expected);
}
