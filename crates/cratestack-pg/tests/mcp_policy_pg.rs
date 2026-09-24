//! cratestack#1038 (MCP phase 3): database-enforced policy over MCP, against
//! real Postgres. Two things only a database can decide:
//!
//! - a delegated `@authorize(McpNote, update, args.id)` denies a caller who
//!   may not update that row, before the implementation runs;
//! - a tool whose implementation reads through the ORM returns no row the
//!   caller's `@@allow('read', ...)` hides, because the policy is in the SQL.
//!
//! Gated `required-features = ["mcp"]`. Run with a database and
//! `CRATESTACK_REQUIRE_DB=1` — `just test-ci-db-mcp` does — or a missing
//! database is a silent skip that still prints `ok` (CLAUDE.md, "Critical
//! test gotcha").

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use cratestack::mcp::StdioServer;
use cratestack::{CratestackContext, CratestackError, Value, include_server_schema};
use serde_json::json;
use support::pg;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

include_server_schema!("tests/fixtures/mcp_policy_pg.cstack", db = Postgres);

use cratestack_schema::procedures::{archive_note, my_notes};

#[derive(Clone, Default)]
struct Procedures {
    runs: Arc<AtomicUsize>,
}

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    async fn my_notes(
        &self,
        db: &cratestack_schema::Cratestack,
        ctx: &CratestackContext,
        _args: my_notes::Args,
        _authorized: my_notes::Authorized,
    ) -> Result<my_notes::Output, CratestackError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        // No filter of its own: whatever the caller may not read has to be
        // kept out by the policy in the SQL.
        db.mcp_note().find_many().run(ctx).await
    }

    async fn archive_note(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        _args: archive_note::Args,
        _authorized: archive_note::Authorized,
    ) -> Result<archive_note::Output, CratestackError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        Ok(true)
    }
}

fn user(id: &str) -> CratestackContext {
    CratestackContext::authenticated([
        ("id".to_owned(), Value::String(id.to_owned())),
        ("role".to_owned(), Value::String("member".to_owned())),
    ])
}

/// One `tools/call` over stdio framing, returning the `result` object.
async fn call(
    db: &cratestack_schema::Cratestack,
    registry: &Procedures,
    ctx: CratestackContext,
    name: &str,
    arguments: serde_json::Value,
) -> serde_json::Value {
    let tools = cratestack_schema::mcp::tools(db.clone(), registry.clone(), ());
    let server = StdioServer::new(tools, ctx).expect("generated table");
    let (mut writer, server_reader) = tokio::io::duplex(1 << 16);
    let (server_writer, reader) = tokio::io::duplex(1 << 16);
    tokio::spawn(server.serve_io(server_reader, server_writer));
    let request = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {
            "name": name, "arguments": arguments,
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
    let line = BufReader::new(reader)
        .lines()
        .next_line()
        .await
        .unwrap()
        .unwrap();
    let response: serde_json::Value = serde_json::from_str(&line).unwrap();
    response["result"].clone()
}

async fn seeded() -> Option<(pg::TestPg, cratestack_schema::Cratestack)> {
    let test_pg = pg::connect_or_skip().await?;
    let pool = &test_pg.pool;
    for statement in [
        "DROP TABLE IF EXISTS mcp_notes",
        "CREATE TABLE mcp_notes (id TEXT PRIMARY KEY, owner_id TEXT NOT NULL, title TEXT NOT NULL)",
        "INSERT INTO mcp_notes (id, owner_id, title) VALUES \
         ('note_a', 'u-1', 'mine'), ('note_b', 'u-2', 'theirs')",
    ] {
        cratestack::sqlx::query(statement)
            .execute(pool)
            .await
            .expect(statement);
    }
    let db = cratestack_schema::Cratestack::builder(pool.clone()).build();
    Some((test_pg, db))
}

/// One test, one container: on rootless Docker a second container start in
/// the same binary intermittently fails in RootlessKit's port manager (the
/// race CLAUDE.md and CI's `facade-disjointness` comments describe), which
/// reads as a test failure. The two checks stay separate functions, each
/// with its own messages, and each seeds nothing the other depends on.
#[tokio::test]
async fn database_enforced_policy_holds_over_mcp() {
    let _guard = pg::serial_guard().await;
    let Some((_pg, db)) = seeded().await else {
        return;
    };
    a_delegated_authorize_denial_refuses_the_call_before_it_runs(&db).await;
    a_row_the_callers_allow_hides_is_absent_from_the_result(&db).await;
}

async fn a_delegated_authorize_denial_refuses_the_call_before_it_runs(
    db: &cratestack_schema::Cratestack,
) {
    let registry = Procedures::default();

    let denied = call(
        db,
        &registry,
        user("u-1"),
        "archiveNote",
        json!({ "args": { "id": "note_b" } }),
    )
    .await;
    assert_eq!(
        denied["isError"],
        json!(true),
        "u-1 may not update u-2's note: {denied}"
    );
    let envelope: serde_json::Value =
        serde_json::from_str(denied["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(envelope["code"], "FORBIDDEN", "{envelope}");
    assert_eq!(
        registry.runs.load(Ordering::SeqCst),
        0,
        "denied before the implementation"
    );

    let allowed = call(
        db,
        &registry,
        user("u-1"),
        "archiveNote",
        json!({ "args": { "id": "note_a" } }),
    )
    .await;
    assert_eq!(allowed["isError"], json!(false), "{allowed}");
    assert_eq!(registry.runs.load(Ordering::SeqCst), 1);
}

async fn a_row_the_callers_allow_hides_is_absent_from_the_result(
    db: &cratestack_schema::Cratestack,
) {
    let registry = Procedures::default();

    for (caller, visible) in [("u-1", "note_a"), ("u-2", "note_b")] {
        let result = call(
            db,
            &registry,
            user(caller),
            "myNotes",
            json!({ "tag": "x" }),
        )
        .await;
        assert_eq!(result["isError"], json!(false), "{result}");
        let rows: serde_json::Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        let ids: Vec<&str> = rows
            .as_array()
            .expect("a list output is a JSON array")
            .iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, [visible], "{caller} sees exactly its own note");
    }
}
