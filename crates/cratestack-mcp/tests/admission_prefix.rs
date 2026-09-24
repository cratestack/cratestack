//! The cratestack#1039 maintainer decision: system and user identities get
//! distinct admission namespaces. `SystemContext::for_service("svc")`'s id
//! is the string `system:svc`, and a token's `id` claim is whatever the
//! token says, so under one shared prefix a token claiming
//! `id = "system:svc"` could replay a result the service recorded.
//!
//! One store and one executor are shared throughout, as a stdio operator
//! process and an HTTP server pointed at one Redis would share them. The
//! system call records under key `k-1`; the impostor then sends the same
//! tool, the same arguments and the same key, which is exactly a replay if
//! the two namespaces coincide.

mod support;

use std::sync::Arc;
use std::time::Duration;

use cratestack_core::{IdempotencyStore, SystemContext};
use cratestack_mcp::{OpExecutor, StdioServer};
use serde_json::{Value, json};
use support::client::{Client, text};
use support::http_app::{RESOURCE, builder, keyed_call, mount, send, token};
use support::stores::MemoryIdempotency;
use support::{FakeTools, user};

fn key(value: &str) -> Option<Value> {
    Some(json!({ "dev.cratestack/idempotencyKey": value }))
}

fn shared_executor() -> OpExecutor {
    let store: Arc<dyn IdempotencyStore> = Arc::new(MemoryIdempotency::default());
    OpExecutor::new(Some(store), Duration::from_secs(60))
}

/// The service's own call, over stdio, recorded under `k-1`.
async fn record_as_system(tools: &FakeTools, executor: &OpExecutor) {
    let ctx = SystemContext::for_service("svc").into_context();
    let server = StdioServer::new(tools.clone(), ctx).unwrap();
    let mut system = Client::start(server.with_executor(executor.clone()));
    let recorded = system
        .call("transfer", json!({ "amount": 5 }), key("k-1"))
        .await;
    assert_eq!(text(&recorded), r#"[5,1,"system:svc"]"#);
    // Positive control: the service's own retry does replay, so the store
    // and key are live and the impostor's "not a replay" below means
    // something.
    let retried = system
        .call("transfer", json!({ "amount": 5 }), key("k-1"))
        .await;
    assert_eq!(
        retried["_meta"]["dev.cratestack/idempotencyReplayed"],
        json!(true)
    );
    assert_eq!(tools.runs(), 1);
}

#[tokio::test]
async fn a_user_claiming_a_system_id_cannot_replay_over_stdio() {
    let tools = FakeTools::default();
    let executor = shared_executor();
    record_as_system(&tools, &executor).await;

    let server = StdioServer::new(tools.clone(), user("system:svc")).unwrap();
    let mut impostor = Client::start(server.with_executor(executor.clone()));
    let result = impostor
        .call("transfer", json!({ "amount": 5 }), key("k-1"))
        .await;

    assert_eq!(tools.runs(), 2, "the impostor's call must run, not replay");
    assert!(result.get("_meta").is_none(), "a live run: {result}");
    assert_eq!(text(&result), r#"[5,2,"system:svc"]"#);
}

#[tokio::test]
async fn a_token_claiming_a_system_id_cannot_replay_over_http() {
    let tools = FakeTools::default();
    let executor = shared_executor();
    record_as_system(&tools, &executor).await;

    let server = builder(&tools, RESOURCE)
        .with_executor(executor.clone())
        .build()
        .unwrap();
    let call = keyed_call(
        "/mcp",
        Some(&token("system:svc")),
        "transfer",
        json!({ "amount": 5 }),
        Some("k-1"),
    );
    let result = send(&mount(&server), call).await.result();

    assert_eq!(tools.runs(), 2, "the token's call must run, not replay");
    assert!(result.get("_meta").is_none(), "a live run: {result}");
    assert_eq!(
        result["content"][0]["text"],
        json!([5, 2, "system:svc"]).to_string()
    );
}
