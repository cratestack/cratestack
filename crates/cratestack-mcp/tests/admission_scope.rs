//! L3 admission over MCP beyond the happy path (cratestack#1038 review):
//! whose namespace a key lives in, what an error outcome records, which
//! identities can be scoped at all, and what a failing rate-limit store
//! does to a call. Each test kills a mutation the suite in
//! `admission.rs` let survive.

mod support;

use std::sync::Arc;
use std::time::Duration;

use cratestack_core::{
    CratestackContext, CratestackError, IdempotencyStore, RateLimitConfig, RateLimitStore, Value,
};
use cratestack_mcp::{OpExecutor, StdioServer};
use serde_json::json;
use support::client::{Client, envelope, text};
use support::failing::FailingLimiter;
use support::stores::{CountingLimiter, MemoryIdempotency};
use support::{FakeTools, user};

fn key(value: &str) -> Option<serde_json::Value> {
    Some(json!({ "dev.cratestack/idempotencyKey": value }))
}

fn idempotent_executor() -> OpExecutor {
    let store: Arc<dyn IdempotencyStore> = Arc::new(MemoryIdempotency::default());
    OpExecutor::new(Some(store), Duration::from_secs(60))
}

fn limited(tools: &FakeTools, ctx: CratestackContext, store: Arc<dyn RateLimitStore>) -> Client {
    let executor =
        OpExecutor::new(None, Duration::ZERO).with_rate_limit(store, RateLimitConfig::new(5, 0.0));
    Client::start(
        StdioServer::new(tools.clone(), ctx)
            .unwrap()
            .with_executor(executor),
    )
}

/// cratestack#416's rule on MCP: two callers sharing one store and one key
/// must not see each other's results. Both servers hold the same executor,
/// as two stdio processes pointed at one Redis would.
#[tokio::test]
async fn one_principal_cannot_replay_anothers_result() {
    let tools = FakeTools::default();
    let executor = idempotent_executor();
    let serve = |id: &str| {
        let server = StdioServer::new(tools.clone(), user(id)).unwrap();
        Client::start(server.with_executor(executor.clone()))
    };
    let mut alice = serve("u-1");
    let mut bob = serve("u-2");

    let first = alice
        .call("transfer", json!({ "amount": 5 }), key("k-1"))
        .await;
    let second = bob
        .call("transfer", json!({ "amount": 5 }), key("k-1"))
        .await;

    assert_eq!(tools.runs(), 2, "u-2's call must run, not replay u-1's");
    assert_eq!(text(&first), r#"[5,1,"u-1"]"#);
    assert_eq!(text(&second), r#"[5,2,"u-2"]"#, "u-2 got its own result");
    assert!(second.get("_meta").is_none(), "a live run, not a replay");
}

/// As on HTTP (`IdempotencyStore::complete`'s contract, `cratestack-axum`'s
/// `buffer_and_persist_response`), the outcome is frozen whatever it was:
/// a keyed call that failed replays the failure rather than running again.
#[tokio::test]
async fn an_error_outcome_is_recorded_and_replayed() {
    let tools = FakeTools::default();
    let server = StdioServer::new(tools.clone(), user("u-1")).unwrap();
    let mut client = Client::start(server.with_executor(idempotent_executor()));

    let first = client
        .call("transfer", json!({ "amount": -1 }), key("k-err"))
        .await;
    let second = client
        .call("transfer", json!({ "amount": -1 }), key("k-err"))
        .await;

    assert_eq!(envelope(&first)["code"], "DATABASE_ERROR");
    assert_eq!(tools.runs(), 1, "the failed call is replayed, not re-run");
    assert_eq!(first["content"], second["content"]);
    assert_eq!(
        second["_meta"]["dev.cratestack/idempotencyReplayed"],
        json!(true)
    );
}

/// An `Int` `id` claim — `auth User { id Int }`, the shape ADR 0002's own
/// examples use (`authorId == auth().id` on an `Int` column) — is an
/// identity like a string one. Refusing it would make every rate-limited
/// call fail with advice to add the `id` claim the caller already has.
#[tokio::test]
async fn an_integer_id_claim_scopes_admission() {
    let tools = FakeTools::default();
    let ctx = CratestackContext::authenticated([("id".to_owned(), Value::Int(7))]);
    let limiter = Arc::new(CountingLimiter::new(5));
    let mut client = limited(&tools, ctx, limiter.clone());

    let result = client.call("echo", json!({ "text": "a" }), None).await;

    assert_eq!(result["isError"], json!(false), "{result}");
    assert_eq!(*limiter.keys.lock().unwrap(), ["mcp:7"]);
}

/// `StoreErrorPolicy::default()` on HTTP serves through only a
/// transport-class failure (`Unavailable`); any other store failure
/// refuses, and the tool must not run.
#[tokio::test]
async fn a_rate_limit_store_failure_other_than_unavailable_refuses() {
    let tools = FakeTools::default();
    let store = Arc::new(FailingLimiter::new(|| {
        CratestackError::Internal("limiter bug".to_owned())
    }));
    let mut client = limited(&tools, user("u-1"), store);

    let result = client.call("echo", json!({ "text": "a" }), None).await;

    assert_eq!(envelope(&result)["code"], "INTERNAL_ERROR");
    assert!(
        !text(&result).contains("limiter bug"),
        "detail stays in the log"
    );
    assert_eq!(tools.runs(), 0);
}

/// The other half of that default: an unreachable store serves the call.
/// This pins parity with HTTP's default, not a judgement that fail-open is
/// right for every deployment — HTTP lets an application choose
/// `StoreErrorPolicy::Deny`, and MCP does not yet (see the review notes).
#[tokio::test]
async fn an_unavailable_rate_limit_store_serves_the_call() {
    let tools = FakeTools::default();
    let store = Arc::new(FailingLimiter::new(|| {
        CratestackError::Unavailable("redis down".to_owned())
    }));
    let mut client = limited(&tools, user("u-1"), store);

    let result = client.call("echo", json!({ "text": "a" }), None).await;

    assert_eq!(result["isError"], json!(false), "{result}");
    assert_eq!(tools.runs(), 1);
}
