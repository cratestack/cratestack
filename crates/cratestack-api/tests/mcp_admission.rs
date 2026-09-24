//! cratestack#1038 (MCP phase 3): L3 admission for generated tools — ADR
//! 0002 Q6's `_meta` idempotency key and the application-configured rate
//! limiter — with the registry's run counter as the witness.
//!
//! Gated `required-features = ["mcp"]`; `just test-ci-host` runs it.

mod mcp_support;

use std::sync::Arc;
use std::time::Duration;

use cratestack::mcp::{OpExecutor, StdioServer};
use cratestack::ratelimit::InMemoryRateLimitStore;
use cratestack::{IdempotencyStore, RateLimitConfig, RateLimitStore};
use mcp_support::client::{Client, envelope, key};
use mcp_support::store::MemoryStore;
use mcp_support::{Registry, caller, tools};
use serde_json::json;

/// The application builds the executor, as it would for
/// `cratestack-axum`'s `IdempotencyLayer`.
fn with_idempotency(registry: &Registry) -> Client {
    let store: Arc<dyn IdempotencyStore> = Arc::new(MemoryStore::default());
    let executor = OpExecutor::new(Some(store), Duration::from_secs(60));
    Client::start(
        StdioServer::new(tools(registry), caller("u-1", "teller"))
            .unwrap()
            .with_executor(executor),
    )
}

/// The decisive idempotency test (ADR 0002 Q6). Removing the `_meta` →
/// `OpInput.idempotency_key` plumbing makes the second call run again,
/// and this fails.
#[tokio::test]
async fn the_same_key_runs_the_tool_once_and_replays_the_first_result() {
    let registry = Registry::default();
    let mut client = with_idempotency(&registry);
    let arguments = json!({ "args": { "amount": 25 } });

    let first = client
        .call("transfer_funds", arguments.clone(), key("pay-1"))
        .await;
    let second = client.call("transfer_funds", arguments, key("pay-1")).await;

    assert_eq!(
        registry.runs(),
        1,
        "the retry must replay, not run the transfer again"
    );
    assert_eq!(
        first["structuredContent"],
        json!({ "amount": 25, "run": 1 })
    );
    assert_eq!(
        second["structuredContent"], first["structuredContent"],
        "the replay is the first call's recorded result"
    );
    assert_eq!(
        second["_meta"]["dev.cratestack/idempotencyReplayed"],
        json!(true)
    );
}

#[tokio::test]
async fn without_a_key_the_same_call_runs_twice() {
    let registry = Registry::default();
    let mut client = with_idempotency(&registry);
    let arguments = json!({ "args": { "amount": 25 } });
    client.call("transfer_funds", arguments.clone(), None).await;
    let second = client.call("transfer_funds", arguments, None).await;
    assert_eq!(registry.runs(), 2, "no key, no reservation");
    assert_eq!(second["structuredContent"]["run"], json!(2));
}

#[tokio::test]
async fn a_no_idempotency_tool_ignores_the_key() {
    let registry = Registry::default();
    let mut client = with_idempotency(&registry);
    client
        .call("touch", json!({ "args": { "amount": 1 } }), key("k"))
        .await;
    client
        .call("touch", json!({ "args": { "amount": 1 } }), key("k"))
        .await;
    assert_eq!(registry.runs(), 2, "`@no_idempotency` takes no reservation");
}

#[tokio::test]
async fn an_exhausted_rate_limit_is_refused_before_the_tool_runs() {
    let registry = Registry::default();
    let limiter: Arc<dyn RateLimitStore> = Arc::new(InMemoryRateLimitStore::new());
    let executor = OpExecutor::new(None, Duration::ZERO)
        .with_rate_limit(limiter, RateLimitConfig::new(1, 0.001));
    let mut client = Client::start(
        StdioServer::new(tools(&registry), caller("u-1", "teller"))
            .unwrap()
            .with_executor(executor),
    );

    let first = client.call("whoami", json!({ "tag": "a" }), None).await;
    assert_eq!(first["isError"], json!(false), "{first}");
    let second = client.call("whoami", json!({ "tag": "a" }), None).await;
    assert_eq!(envelope(&second)["code"], "TOO_MANY_REQUESTS");
    assert_eq!(registry.runs(), 1, "the throttled call never ran");
}
