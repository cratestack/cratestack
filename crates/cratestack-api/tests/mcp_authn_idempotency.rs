//! cratestack#1261 over MCP: a step-up refusal of the generated authorize
//! step comes before the tool's body, so `admit_and_run` releases the `_meta`
//! idempotency key instead of recording the refusal, and the caller who
//! stepped up runs the tool once under the same key. A refusal the body
//! returns after a write is recorded and replayed.
//!
//! MCP authenticates before admission (the namespace is the context's
//! subject), so an `authenticate` refusal never reaches a reservation here.
//! Each caller is its own stdio server, sharing one executor and so one
//! store: the same subject, before and after the step-up.
//!
//! Gated `required-features = ["mcp"]`.

#[allow(dead_code)] // Only `call`, `envelope` and `key` are used here.
#[path = "mcp_support/client.rs"]
mod client;
#[path = "mcp_support/store.rs"]
mod store;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use client::{Client, envelope, key};
use cratestack::mcp::{OpExecutor, StdioServer};
use cratestack::{
    AuthRefusal, CratestackContext, CratestackError, IdempotencyStore, Value, include_server_schema,
};
use serde_json::json;
use store::MemoryStore;

include_server_schema!("tests/fixtures/mcp_authn_idempotency.cstack", db = None);

#[derive(Clone, Default)]
struct Registry(Arc<AtomicUsize>);

impl Registry {
    fn runs(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

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

    async fn refund(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        _args: cratestack_schema::procedures::refund::Args,
        _authorized: cratestack_schema::procedures::refund::Authorized,
    ) -> Result<cratestack_schema::Receipt, CratestackError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        // Propagated from a nested generated call, so already claimed: the
        // body's `invoke_with_db` must still disown it.
        Err(AuthRefusal::unauthenticated("signing key rotated")
            .with_member("error", Value::String("key_rotated".into()))
            .into_error()
            .__generated_claim_auth_refusal())
    }
}

#[derive(Clone)]
struct Resolver;

impl cratestack_schema::ComputedFieldResolver for Resolver {}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn stale() -> CratestackContext {
    CratestackContext::builder("usr-1")
        .auth_time(now() - 3600)
        .amr(["pwd"])
        .build()
}

fn fresh() -> CratestackContext {
    CratestackContext::builder("usr-1")
        .auth_time(now())
        .amr(["hwk", "user"])
        .build()
}

fn executor() -> OpExecutor {
    let store: Arc<dyn IdempotencyStore> = Arc::new(MemoryStore::default());
    OpExecutor::new(Some(store), Duration::from_secs(60))
}

fn serve(registry: &Registry, executor: &OpExecutor, ctx: CratestackContext) -> Client {
    let tools = cratestack_schema::mcp::tools(
        cratestack_schema::Cratestack::builder().build(),
        registry.clone(),
        Resolver,
    );
    Client::start(
        StdioServer::new(tools, ctx)
            .expect("generated table is valid")
            .with_executor(executor.clone()),
    )
}

#[tokio::test]
async fn a_step_up_refusal_is_not_recorded_under_the_key() {
    let (registry, executor) = (Registry::default(), executor());
    let arguments = json!({ "args": { "amount": 5 } });

    let refused = serve(&registry, &executor, stale())
        .call("pay", arguments.clone(), key("pay-1"))
        .await;
    assert_eq!(
        envelope(&refused)["error"],
        "insufficient_user_authentication"
    );
    assert_eq!(registry.runs(), 0);

    let mut stepped_up = serve(&registry, &executor, fresh());
    let served = stepped_up
        .call("pay", arguments.clone(), key("pay-1"))
        .await;
    assert_eq!(
        served["structuredContent"],
        json!({ "ok": true }),
        "{served}"
    );
    assert!(
        served["_meta"]["dev.cratestack/idempotencyReplayed"].is_null(),
        "the refusal must not have been recorded: {served}"
    );
    assert_eq!(registry.runs(), 1);

    let again = stepped_up.call("pay", arguments, key("pay-1")).await;
    assert_eq!(
        again["_meta"]["dev.cratestack/idempotencyReplayed"],
        json!(true)
    );
    assert_eq!(registry.runs(), 1, "the key runs `pay` exactly once");
}

#[tokio::test]
async fn a_refusal_from_the_body_is_recorded_under_the_key() {
    let (registry, executor) = (Registry::default(), executor());
    let arguments = json!({ "args": { "amount": 5 } });

    let refused = serve(&registry, &executor, stale())
        .call("refund", arguments.clone(), key("refund-1"))
        .await;
    assert_eq!(envelope(&refused)["error"], "key_rotated");
    assert_eq!(registry.runs(), 1, "the body ran and wrote");

    let again = serve(&registry, &executor, fresh())
        .call("refund", arguments, key("refund-1"))
        .await;
    assert_eq!(envelope(&again)["error"], "key_rotated");
    assert_eq!(
        again["_meta"]["dev.cratestack/idempotencyReplayed"],
        json!(true)
    );
    assert_eq!(registry.runs(), 1, "the write is never run twice");
}
