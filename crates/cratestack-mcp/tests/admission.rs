//! L3 admission over MCP (ADR 0002 D2, Q6), with the fake table's run
//! counter as the witness: a refused call must never run, a replay must not
//! run again. The generated-table versions of the decisive cases live in
//! `cratestack-api`'s `tests/mcp_admission.rs`.

mod support;

use std::sync::Arc;
use std::time::Duration;

use cratestack_core::{CratestackContext, IdempotencyStore, RateLimitConfig, RateLimitStore};
use cratestack_mcp::{OpExecutor, StdioServer};
use serde_json::json;
use support::client::{Client, envelope};
use support::stores::{CountingLimiter, MemoryIdempotency, bucket};
use support::{FakeTools, user, without_id};

fn with_store(tools: FakeTools, ctx: CratestackContext) -> Client {
    let store: Arc<dyn IdempotencyStore> = Arc::new(MemoryIdempotency::default());
    let executor = OpExecutor::new(Some(store), Duration::from_secs(60));
    Client::start(
        StdioServer::new(tools, ctx)
            .unwrap()
            .with_executor(executor),
    )
}

fn key(value: &str) -> Option<serde_json::Value> {
    Some(json!({ "dev.cratestack/idempotencyKey": value }))
}

#[tokio::test]
async fn the_same_key_runs_once_and_replays_the_first_result() {
    let tools = FakeTools::default();
    let mut client = with_store(tools.clone(), user("u-1"));
    let first = client
        .call("transfer", json!({ "amount": 5 }), key("k-1"))
        .await;
    let second = client
        .call("transfer", json!({ "amount": 5 }), key("k-1"))
        .await;

    assert_eq!(tools.runs(), 1, "the second call must replay, not run");
    assert_eq!(
        first["content"], second["content"],
        "the recorded result, byte for byte"
    );
    assert_eq!(
        second["_meta"]["dev.cratestack/idempotencyReplayed"],
        json!(true)
    );
    assert!(first.get("_meta").is_none(), "a live run is not marked");
    client.close().await.unwrap();
}

#[tokio::test]
async fn without_a_key_nothing_is_reserved() {
    let tools = FakeTools::default();
    let mut client = with_store(tools.clone(), user("u-1"));
    let first = client.call("transfer", json!({ "amount": 5 }), None).await;
    let second = client.call("transfer", json!({ "amount": 5 }), None).await;
    assert_eq!(tools.runs(), 2);
    assert_ne!(first["content"], second["content"], "two live runs");
    client.close().await.unwrap();
}

#[tokio::test]
async fn a_reused_key_with_other_arguments_is_a_conflict() {
    let tools = FakeTools::default();
    let mut client = with_store(tools.clone(), user("u-1"));
    client
        .call("transfer", json!({ "amount": 5 }), key("k-1"))
        .await;
    let reused = client
        .call("transfer", json!({ "amount": 6 }), key("k-1"))
        .await;
    let error = envelope(&reused);
    assert_eq!(error["code"], "VALIDATION_ERROR");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .starts_with("idempotency_key_conflict")
    );
    assert_eq!(tools.runs(), 1);
    client.close().await.unwrap();
}

#[tokio::test]
async fn a_read_tool_never_reserves_even_with_a_key() {
    let tools = FakeTools::default();
    // No `id` claim: if a reservation were attempted, the missing principal
    // id would refuse the call. It runs, so none was.
    let mut client = with_store(tools.clone(), without_id());
    client
        .call("echo", json!({ "text": "a" }), key("k-1"))
        .await;
    client
        .call("echo", json!({ "text": "a" }), key("k-1"))
        .await;
    assert_eq!(tools.runs(), 2);
    client.close().await.unwrap();
}

#[tokio::test]
async fn a_keyed_mutation_without_a_principal_id_is_refused_before_running() {
    let tools = FakeTools::default();
    let mut client = with_store(tools.clone(), without_id());
    let result = client
        .call("transfer", json!({ "amount": 5 }), key("k-1"))
        .await;
    assert_eq!(envelope(&result)["code"], "PRECONDITION_FAILED");
    assert_eq!(tools.runs(), 0);
    client.close().await.unwrap();
}

#[tokio::test]
async fn a_malformed_key_is_refused_not_ignored() {
    let tools = FakeTools::default();
    let mut client = with_store(tools.clone(), user("u-1"));
    let result = client
        .call(
            "transfer",
            json!({ "amount": 5 }),
            Some(json!({ "dev.cratestack/idempotencyKey": 42 })),
        )
        .await;
    assert_eq!(envelope(&result)["code"], "BAD_REQUEST");
    assert_eq!(tools.runs(), 0);
    client.close().await.unwrap();
}

#[tokio::test]
async fn an_exhausted_budget_is_too_many_requests_and_nothing_runs() {
    let tools = FakeTools::default();
    let limiter = Arc::new(CountingLimiter::new(1));
    let executor = OpExecutor::new(None, Duration::ZERO).with_rate_limit(
        limiter.clone() as Arc<dyn RateLimitStore>,
        RateLimitConfig::new(1, 0.0),
    );
    let server = StdioServer::new(tools.clone(), user("u-1"))
        .unwrap()
        .with_executor(executor);
    let mut client = Client::start(server);

    let allowed = client.call("echo", json!({ "text": "a" }), None).await;
    assert_eq!(allowed["isError"], json!(false));
    let throttled = client.call("echo", json!({ "text": "a" }), None).await;
    assert_eq!(envelope(&throttled)["code"], "TOO_MANY_REQUESTS");

    assert_eq!(tools.runs(), 1, "the throttled call never ran");
    assert_eq!(
        *limiter.keys.lock().unwrap(),
        [bucket("mcp", "u-1"), bucket("mcp", "u-1")]
    );
    client.close().await.unwrap();
}

#[tokio::test]
async fn without_an_executor_nothing_is_limited() {
    let tools = FakeTools::default();
    let mut client = Client::start(StdioServer::new(tools.clone(), user("u-1")).unwrap());
    for _ in 0..3 {
        client
            .call("transfer", json!({ "amount": 1 }), key("k"))
            .await;
    }
    assert_eq!(tools.runs(), 3, "no store wired: the key reserves nothing");
    client.close().await.unwrap();
}
