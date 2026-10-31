//! `@authn` over MCP (ADR 0022, cratestack#1244): the tool call goes through
//! the same generated authorize step, so a caller who needs a step-up gets
//! an `isError` result carrying the same envelope (`UNAUTHORIZED`, the
//! `error` member, `details`), and the implementation never runs.
//!
//! Gated `required-features = ["mcp"]`.

#[allow(dead_code)] // Only `call` and `envelope` are used here.
#[path = "mcp_support/client.rs"]
mod client;

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use client::{Client, envelope};
use cratestack::mcp::StdioServer;
use cratestack::{CratestackContext, CratestackError, include_server_schema};
use serde_json::json;

include_server_schema!("tests/fixtures/mcp_authn.cstack", db = None);

#[derive(Clone, Default)]
struct Registry(Arc<AtomicI64>);

impl cratestack_schema::procedures::ProcedureRegistry for Registry {
    async fn pay(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        _args: cratestack_schema::procedures::pay::Args,
        _authorized: cratestack_schema::procedures::pay::Authorized,
    ) -> Result<cratestack_schema::Receipt, CratestackError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(cratestack_schema::Receipt { ok: true })
    }
}

#[derive(Clone)]
struct Resolver;

impl cratestack_schema::ComputedFieldResolver for Resolver {}

fn serve(registry: &Registry, ctx: CratestackContext) -> Client {
    let tools = cratestack_schema::mcp::tools(
        cratestack_schema::Cratestack::builder().build(),
        registry.clone(),
        Resolver,
    );
    Client::start(StdioServer::new(tools, ctx).expect("generated table is valid"))
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

#[tokio::test]
async fn a_tool_call_that_needs_a_step_up_is_refused_and_never_runs() {
    let registry = Registry::default();
    let stale = CratestackContext::builder("usr-1")
        .auth_time(now() - 3600)
        .amr(["pwd"])
        .build();
    let refused = serve(&registry, stale)
        .call("pay", json!({ "args": { "amount": 5 } }), None)
        .await;
    let error = envelope(&refused);
    assert_eq!(error["code"], "UNAUTHORIZED", "{error}");
    assert_eq!(error["error"], "insufficient_user_authentication");
    assert_eq!(
        error["details"],
        json!({ "max_age": 300, "amr": ["hwk"], "unmet": ["max_age", "amr"] })
    );
    assert_eq!(
        registry.0.load(Ordering::SeqCst),
        0,
        "the tool must not run"
    );

    let fresh = CratestackContext::builder("usr-1")
        .auth_time(now())
        .amr(["hwk", "user"])
        .build();
    let served = serve(&registry, fresh)
        .call("pay", json!({ "args": { "amount": 5 } }), None)
        .await;
    assert_eq!(
        served["structuredContent"],
        json!({ "ok": true }),
        "{served}"
    );
    assert_eq!(registry.0.load(Ordering::SeqCst), 1);
}
