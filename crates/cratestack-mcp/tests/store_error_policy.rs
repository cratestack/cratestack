//! cratestack#1038 maintainer decision 1: MCP honours the application's
//! `StoreErrorPolicy`, the one type HTTP's `RateLimitLayer` takes too.
//! Before it, MCP hard-coded HTTP's *default*, so an application that chose
//! `Deny` on HTTP was fail-open over MCP.
//!
//! The decisive case is the first test: under `Deny`, an `Unavailable`
//! store refuses the call and the tool never runs. Making the server ignore
//! the configured policy (serving through `Unavailable` regardless) makes it
//! fail. The default's half — an unreachable store serves — is pinned both
//! here and in `admission_scope.rs`.

mod support;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use cratestack_core::{CratestackContext, CratestackError, RateLimitConfig, RateLimitStore};
use cratestack_mcp::{OpExecutor, StdioServer, StoreErrorPolicy};
use serde_json::json;
use support::client::{Client, envelope};
use support::failing::{FailingLimiter, HungLimiter};
use support::{FakeTools, user};

fn executor(store: Arc<dyn RateLimitStore>) -> OpExecutor {
    OpExecutor::new(None, Duration::ZERO).with_rate_limit(store, RateLimitConfig::new(5, 0.0))
}

/// `policy: None` leaves the builder untouched, so the default is what the
/// server itself picks, not a value this test restates.
fn serve(
    tools: &FakeTools,
    ctx: CratestackContext,
    store: Arc<dyn RateLimitStore>,
    policy: Option<StoreErrorPolicy>,
) -> Client {
    let mut server = StdioServer::new(tools.clone(), ctx).unwrap();
    // Set before `with_executor` on purpose: installing the executor must
    // not reset the policy.
    if let Some(policy) = policy {
        server = server.with_store_error_policy(policy);
    }
    Client::start(server.with_executor(executor(store)))
}

fn unavailable() -> Arc<FailingLimiter> {
    Arc::new(FailingLimiter::new(|| {
        CratestackError::Unavailable("redis down".to_owned())
    }))
}

#[tokio::test]
async fn deny_refuses_an_unavailable_store_and_the_tool_never_runs() {
    let tools = FakeTools::default();
    let mut client = serve(
        &tools,
        user("u-1"),
        unavailable(),
        Some(StoreErrorPolicy::Deny),
    );

    let result = client.call("echo", json!({ "text": "a" }), None).await;

    assert_eq!(envelope(&result)["code"], "UNAVAILABLE", "{result}");
    assert_eq!(
        tools.runs(),
        0,
        "a Deny'd store failure must not run the tool"
    );
}

/// A hung store is the same failure after `DEFAULT_STORE_TIMEOUT`: `Deny`
/// refuses it too, rather than serving once the budget elapses.
#[tokio::test]
async fn deny_refuses_a_hung_store_once_the_budget_elapses() {
    let tools = FakeTools::default();
    let store = Arc::new(HungLimiter::default());
    let mut client = serve(
        &tools,
        user("u-1"),
        store.clone(),
        Some(StoreErrorPolicy::Deny),
    );

    let result = tokio::time::timeout(
        Duration::from_secs(3),
        client.call("echo", json!({ "text": "a" }), None),
    )
    .await
    .expect("a hung rate-limit store must not hang the tool call");

    assert_eq!(envelope(&result)["code"], "UNAVAILABLE", "{result}");
    assert_eq!(store.calls.load(Ordering::SeqCst), 1);
    assert_eq!(tools.runs(), 0);
}

#[tokio::test]
async fn the_default_serves_through_an_unavailable_store() {
    let tools = FakeTools::default();
    let mut client = serve(&tools, user("u-1"), unavailable(), None);

    let result = client.call("echo", json!({ "text": "a" }), None).await;

    assert_eq!(result["isError"], json!(false), "{result}");
    assert_eq!(tools.runs(), 1);
}

#[tokio::test]
async fn an_explicit_allow_behaves_as_the_default() {
    let tools = FakeTools::default();
    let mut client = serve(
        &tools,
        user("u-1"),
        unavailable(),
        Some(StoreErrorPolicy::Allow),
    );

    let result = client.call("echo", json!({ "text": "a" }), None).await;

    assert_eq!(result["isError"], json!(false), "{result}");
    assert_eq!(tools.runs(), 1);
}
