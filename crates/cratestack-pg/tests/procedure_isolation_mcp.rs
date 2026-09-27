//! GHSA-r67q-4qqq-g9gm over MCP: a `tools/call` of an `@isolation` tool
//! runs at the declared level — the MCP `execute` arm goes through the
//! same transaction-running `invoke_with_db` REST and RPC use
//! (docs/design/procedure-isolation.md §2). Its exhausted retries carry
//! `TRANSACTION_ABORTED` and are not recorded under an idempotency key (§5),
//! and its `@computed` output is resolved inside the attempt (§6).
//! REST/RPC: `procedure_isolation.rs`, `procedure_isolation_outcome.rs`.
//!
//! Gated `required-features = ["mcp"]`. Run with a database and
//! `CRATESTACK_REQUIRE_DB=1` — `just test-ci-db-mcp` does — or a missing
//! database is a silent skip that still prints `ok`.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use cratestack::mcp::{OpExecutor, StdioServer};
use cratestack::sqlx;
use cratestack::{
    CratestackContext, CratestackError, IdempotencyStore, Value, include_server_schema,
};
use serde_json::json;
use support::pg;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

include_server_schema!(
    "tests/fixtures/procedure_isolation_mcp.cstack",
    db = Postgres
);

use cratestack_schema::procedures as p;
use cratestack_schema::{Cratestack, Debited, IsolatedCratestack, McpIsoVault, Report};

const LEVEL_SQL: &str = "SELECT current_setting('transaction_isolation')";

/// `McpIsoVault.secret`, which is `@server_only`.
const VAULT_SECRET: &str = "HUNTER2-server-only";

fn db_err(error: sqlx::Error) -> CratestackError {
    cratestack::cratestack_error_from_sqlx(error)
}

async fn level(db: &IsolatedCratestack) -> Result<Report, CratestackError> {
    let level = db
        .transaction(async |tx| {
            sqlx::query_scalar::<_, String>(LEVEL_SQL)
                .fetch_one(&mut ***tx)
                .await
                .map_err(db_err)
        })
        .await?;
    Ok(Report { level })
}

#[derive(Default)]
struct Shared {
    runs: AtomicUsize,
    fail_resolver: AtomicBool,
}

#[derive(Clone, Default)]
struct Procedures(Arc<Shared>);

#[derive(Clone)]
struct Resolvers(Arc<Shared>);

impl p::ProcedureRegistry for Procedures {
    async fn level_serializable(
        &self,
        db: &IsolatedCratestack,
        _ctx: &CratestackContext,
        _args: p::level_serializable::Args,
        _authorized: p::level_serializable::Authorized,
    ) -> Result<Report, CratestackError> {
        level(db).await
    }

    async fn level_repeatable_read(
        &self,
        db: &IsolatedCratestack,
        _ctx: &CratestackContext,
        _args: p::level_repeatable_read::Args,
        _authorized: p::level_repeatable_read::Authorized,
    ) -> Result<Report, CratestackError> {
        level(db).await
    }

    async fn level_plain(
        &self,
        db: &Cratestack,
        _ctx: &CratestackContext,
        _args: p::level_plain::Args,
        _authorized: p::level_plain::Authorized,
    ) -> Result<Report, CratestackError> {
        let level = sqlx::query_scalar::<_, String>(LEVEL_SQL)
            .fetch_one(db.pool())
            .await
            .map_err(db_err)?;
        Ok(Report { level })
    }

    async fn always_conflict(
        &self,
        db: &IsolatedCratestack,
        _ctx: &CratestackContext,
        _args: p::always_conflict::Args,
        _authorized: p::always_conflict::Authorized,
    ) -> Result<Report, CratestackError> {
        self.0.runs.fetch_add(1, Ordering::SeqCst);
        db.transaction(async |tx| {
            sqlx::query(
                "DO $$ BEGIN RAISE EXCEPTION 'forced' USING ERRCODE = 'serialization_failure'; \
                 END $$",
            )
            .execute(&mut ***tx)
            .await
            .map_err(db_err)?;
            Ok(Report {
                level: "unreachable".into(),
            })
        })
        .await
    }

    async fn refuse(
        &self,
        _db: &IsolatedCratestack,
        _ctx: &CratestackContext,
        _args: p::refuse::Args,
        _authorized: p::refuse::Authorized,
    ) -> Result<Report, CratestackError> {
        self.0.runs.fetch_add(1, Ordering::SeqCst);
        Err(CratestackError::Validation("refused".into()))
    }

    async fn debit_checked(
        &self,
        db: &IsolatedCratestack,
        ctx: &CratestackContext,
        args: p::debit_checked::Args,
        _authorized: p::debit_checked::Authorized,
    ) -> Result<Debited, CratestackError> {
        let debit = args.args;
        let account = db
            .mcp_iso_account()
            .find_unique(debit.accountId)
            .run(ctx)
            .await?
            .ok_or_else(|| CratestackError::NotFound("no account".into()))?;
        let updated = db
            .mcp_iso_account()
            .update(debit.accountId)
            .set(cratestack_schema::UpdateMcpIsoAccountInput {
                balance: Some(account.balance - debit.amount),
            })
            .run(ctx)
            .await?;
        Ok(Debited {
            accountId: debit.accountId,
            after: updated.balance,
        })
    }

    async fn open_vault(
        &self,
        _db: &IsolatedCratestack,
        _ctx: &CratestackContext,
        _args: p::open_vault::Args,
        _authorized: p::open_vault::Authorized,
    ) -> Result<McpIsoVault, CratestackError> {
        Ok(McpIsoVault {
            id: 1,
            label: "vault-1".into(),
            secret: VAULT_SECRET.into(),
        })
    }
}

impl cratestack_schema::ComputedFieldResolver for Resolvers {
    fn resolve_debited_checked(
        &self,
        db: &Cratestack,
        source: &Debited,
        ctx: &CratestackContext,
    ) -> impl core::future::Future<Output = Result<i64, CratestackError>> + Send {
        let (db, id, ctx, shared) = (db.clone(), source.accountId, ctx.clone(), self.0.clone());
        async move {
            if shared.fail_resolver.load(Ordering::SeqCst) {
                return Err(CratestackError::Validation("resolver refused".into()));
            }
            // Reads the row the body just wrote: the same transaction.
            let account = db
                .mcp_iso_account()
                .find_unique(id)
                .run(&ctx)
                .await?
                .ok_or_else(|| CratestackError::NotFound("no account".into()))?;
            Ok(account.balance)
        }
    }

    /// `<transaction level>/<secret length>`: proves the output was composed
    /// inside the attempt, and is derived from the `@server_only` value
    /// without being it.
    fn resolve_mcp_iso_vault_hint(
        &self,
        db: &Cratestack,
        source: &McpIsoVault,
        _ctx: &CratestackContext,
    ) -> impl core::future::Future<Output = Result<String, CratestackError>> + Send {
        let (db, length) = (db.clone(), source.secret.len());
        async move {
            let level = db
                .transaction(async |tx| {
                    sqlx::query_scalar::<_, String>(LEVEL_SQL)
                        .fetch_one(&mut ***tx)
                        .await
                        .map_err(db_err)
                })
                .await?;
            Ok(format!("{level}/{length}"))
        }
    }
}

/// A stdio MCP server over the generated table, optionally with an
/// idempotency store; `call` sends one `tools/call` and returns `result`.
struct Mcp {
    writer: tokio::io::DuplexStream,
    lines: tokio::io::Lines<BufReader<tokio::io::DuplexStream>>,
    next_id: u64,
}

impl Mcp {
    fn start(
        db: &Cratestack,
        shared: &Arc<Shared>,
        store: Option<Arc<dyn IdempotencyStore>>,
    ) -> Self {
        let ctx =
            CratestackContext::authenticated([("id".to_owned(), Value::String("u-1".into()))]);
        let tools = cratestack_schema::mcp::tools(
            db.clone(),
            Procedures(shared.clone()),
            Resolvers(shared.clone()),
        );
        let mut server = StdioServer::new(tools, ctx).expect("generated table");
        if let Some(store) = store {
            server = server.with_executor(OpExecutor::new(Some(store), Duration::from_secs(60)));
        }
        let (writer, server_reader) = tokio::io::duplex(1 << 16);
        let (server_writer, reader) = tokio::io::duplex(1 << 16);
        tokio::spawn(server.serve_io(server_reader, server_writer));
        Self {
            writer,
            lines: BufReader::new(reader).lines(),
            next_id: 1,
        }
    }

    async fn call(
        &mut self,
        name: &str,
        arguments: serde_json::Value,
        key: Option<&str>,
    ) -> serde_json::Value {
        let mut meta = json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientCapabilities": {},
        });
        if let Some(key) = key {
            meta["dev.cratestack/idempotencyKey"] = json!(key);
        }
        let request = json!({
            "jsonrpc": "2.0", "id": self.next_id, "method": "tools/call",
            "params": { "name": name, "arguments": arguments, "_meta": meta },
        });
        self.next_id += 1;
        self.writer
            .write_all(format!("{request}\n").as_bytes())
            .await
            .unwrap();
        let line = self.lines.next_line().await.unwrap().unwrap();
        let response: serde_json::Value = serde_json::from_str(&line).unwrap();
        response["result"].clone()
    }
}

fn probe() -> serde_json::Value {
    json!({ "args": { "nonce": "x" } })
}

/// The REST error envelope an `isError` result carries as its text.
fn envelope(result: &serde_json::Value) -> serde_json::Value {
    assert_eq!(result["isError"], json!(true), "{result}");
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[tokio::test]
async fn an_isolated_tool_runs_at_its_declared_level() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let db = Cratestack::builder(test_pg.pool.clone()).build();
    let mut mcp = Mcp::start(&db, &Arc::default(), None);
    for (tool, expected) in [
        ("levelSerializable", "serializable"),
        ("levelRepeatableRead", "repeatable read"),
        ("levelPlain", "read committed"),
    ] {
        let result = mcp.call(tool, probe(), None).await;
        println!("MCP {tool}: {result}");
        assert_eq!(result["isError"], json!(false), "{tool}: {result}");
        let report: serde_json::Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(report["level"], expected, "{tool}: {report}");
    }
}

/// Retries exhausted is `TRANSACTION_ABORTED`, and the same idempotency key
/// runs the tool again rather than replaying it. A validation error under a
/// key is recorded and replayed: the store is live.
#[tokio::test]
async fn an_aborted_tool_call_is_not_recorded_under_its_key() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    let store = cratestack::SqlxIdempotencyStore::new(pool.clone());
    store.ensure_schema().await.expect("idempotency table");
    sqlx::query("DELETE FROM cratestack_idempotency")
        .execute(&pool)
        .await
        .unwrap();
    let db = Cratestack::builder(pool.clone())
        .with_isolation_max_retries(0)
        .build();
    let shared = Arc::new(Shared::default());
    let mut mcp = Mcp::start(&db, &shared, Some(Arc::new(store)));

    let first = mcp.call("alwaysConflict", probe(), Some("k-aborted")).await;
    let second = mcp.call("alwaysConflict", probe(), Some("k-aborted")).await;
    println!("MCP alwaysConflict x2: {first} {second}");
    assert_eq!(envelope(&first)["code"], "TRANSACTION_ABORTED", "{first}");
    assert_eq!(envelope(&second)["code"], "TRANSACTION_ABORTED", "{second}");
    assert!(second.get("_meta").is_none(), "not a replay: {second}");
    assert_eq!(
        shared.runs.load(Ordering::SeqCst),
        2,
        "the same key ran again"
    );

    let first = mcp.call("refuse", probe(), Some("k-refused")).await;
    let second = mcp.call("refuse", probe(), Some("k-refused")).await;
    println!("MCP refuse x2: {first} {second}");
    assert_eq!(envelope(&first)["code"], "VALIDATION_ERROR");
    assert_eq!(
        second["_meta"]["dev.cratestack/idempotencyReplayed"],
        json!(true),
        "{second}"
    );
    assert_eq!(shared.runs.load(Ordering::SeqCst), 3, "replayed, not rerun");

    // cratestack#1033: the reservation, its release and the recorded row all
    // use `mcp:<sha256 hex of "u-1">`. The aborted key's row is gone; the
    // recorded one is under the hashed namespace, never the id verbatim.
    const HASHED_U1: &str = "mcp:a24a7f55f278dd49fb1f99c5507800cb198a5bfe10fe2126cd0b25672152b0da";
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT principal_fingerprint, key FROM cratestack_idempotency ORDER BY key",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows, [(HASHED_U1.to_owned(), "k-refused".to_owned())]);
}

/// The `@computed` field is resolved inside the attempt: it reads the body's
/// own uncommitted write, and when the resolver fails the debit is rolled
/// back.
#[tokio::test]
async fn an_isolated_tools_computed_output_is_resolved_in_the_attempt() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    for statement in [
        "DROP TABLE IF EXISTS mcp_iso_accounts",
        "CREATE TABLE mcp_iso_accounts (id BIGINT PRIMARY KEY, balance BIGINT NOT NULL)",
        "INSERT INTO mcp_iso_accounts VALUES (1, 100)",
    ] {
        sqlx::query(sqlx::AssertSqlSafe(statement.to_owned()))
            .execute(&pool)
            .await
            .unwrap();
    }
    let db = Cratestack::builder(pool.clone()).build();
    let shared = Arc::new(Shared::default());
    let mut mcp = Mcp::start(&db, &shared, None);
    let debit = json!({ "args": { "accountId": 1, "amount": 10 } });

    let result = mcp.call("debitChecked", debit.clone(), None).await;
    println!("MCP debitChecked: {result}");
    assert_eq!(result["isError"], json!(false), "{result}");
    let output: serde_json::Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        (output["after"].clone(), output["checked"].clone()),
        (json!(90), json!(90))
    );

    shared.fail_resolver.store(true, Ordering::SeqCst);
    let failed = mcp.call("debitChecked", debit, None).await;
    println!("MCP debitChecked, failing resolver: {failed}");
    assert_eq!(envelope(&failed)["code"], "VALIDATION_ERROR", "{failed}");
    let balance: i64 = sqlx::query_scalar("SELECT balance FROM mcp_iso_accounts WHERE id = 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(balance, 90, "the failed attempt's debit was rolled back");
}

/// The isolated arm composes a model with a `@computed` and a `@server_only`
/// field inside the attempt (the hint reports `serializable`), and the tool
/// result carries neither the `@server_only` value nor its key.
#[tokio::test]
async fn an_isolated_tools_output_never_carries_a_server_only_value() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let db = Cratestack::builder(test_pg.pool.clone()).build();
    let mut mcp = Mcp::start(&db, &Arc::default(), None);
    let result = mcp.call("openVault", probe(), None).await;
    println!("MCP openVault: {result}");
    assert_eq!(result["isError"], json!(false), "{result}");
    let text = result.to_string();
    assert!(!text.contains(VAULT_SECRET), "leaked the value: {text}");
    let output: serde_json::Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(output.get("secret").is_none(), "leaked the key: {output}");
    assert_eq!(output["label"], "vault-1", "{output}");
    assert_eq!(
        output["hint"],
        format!("serializable/{}", VAULT_SECRET.len()),
        "{output}"
    );
}
