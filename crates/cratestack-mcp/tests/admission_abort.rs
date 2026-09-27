//! GHSA-r67q-4qqq-g9gm under cratestack#1033's hashed namespace: when an
//! `@isolation` tool's own dispatch answers `TRANSACTION_ABORTED`, the
//! reservation is given back under the **same** `mcp:<sha256 of id>`
//! namespace it was taken under, so the same key runs the call again; an
//! abort the tool did not claim is recorded there and replayed. The store's
//! journal is the witness for the namespace, the fake table's run counter
//! for "ran again" versus "replayed".

mod support;

use std::sync::Arc;
use std::time::Duration;

use cratestack_core::{CratestackContext, IdempotencyStore, SystemContext};
use cratestack_mcp::{OpExecutor, StdioServer};
use serde_json::json;
use support::client::{Client, envelope};
use support::stores::{MemoryIdempotency, bucket};
use support::{FakeTools, user};

fn start(tools: FakeTools, ctx: CratestackContext) -> (Client, Arc<MemoryIdempotency>) {
    let store = Arc::new(MemoryIdempotency::default());
    let dyn_store: Arc<dyn IdempotencyStore> = store.clone();
    let executor = OpExecutor::new(Some(dyn_store), Duration::from_secs(60));
    let server = StdioServer::new(tools, ctx)
        .unwrap()
        .with_executor(executor);
    (Client::start(server), store)
}

fn key(value: &str) -> Option<serde_json::Value> {
    Some(json!({ "dev.cratestack/idempotencyKey": value }))
}

fn entry(verb: &'static str, namespace: &str, key: &str) -> (&'static str, String, String) {
    (verb, namespace.to_owned(), key.to_owned())
}

/// 409 is the fake table's claimed abort (the tool's own retries ran out),
/// 408 another procedure's abort the tool propagated.
async fn owner_and_non_owner(ctx: CratestackContext, namespace: &str, raw_id: &str) {
    let tools = FakeTools::default();
    let (mut client, store) = start(tools.clone(), ctx);

    let first = client
        .call("transfer", json!({ "amount": 409 }), key("k-owned"))
        .await;
    assert_eq!(envelope(&first)["code"], "TRANSACTION_ABORTED", "{first}");
    assert!(
        !store.holds(namespace, "k-owned"),
        "the owner's abort gave the key back"
    );
    let again = client
        .call("transfer", json!({ "amount": 409 }), key("k-owned"))
        .await;
    assert_eq!(envelope(&again)["code"], "TRANSACTION_ABORTED", "{again}");
    assert!(again.get("_meta").is_none(), "ran, not replayed: {again}");
    assert_eq!(tools.runs(), 2, "the same key ran the body again");

    let first = client
        .call("transfer", json!({ "amount": 408 }), key("k-foreign"))
        .await;
    assert_eq!(envelope(&first)["code"], "INTERNAL_ERROR", "{first}");
    assert!(store.holds(namespace, "k-foreign"), "recorded");
    let replayed = client
        .call("transfer", json!({ "amount": 408 }), key("k-foreign"))
        .await;
    assert_eq!(envelope(&replayed)["code"], "INTERNAL_ERROR");
    assert_eq!(
        replayed["_meta"]["dev.cratestack/idempotencyReplayed"],
        json!(true),
        "{replayed}"
    );
    assert_eq!(tools.runs(), 3, "the non-owner's abort replayed");

    let journal = store.journal.lock().unwrap().clone();
    assert_eq!(
        journal,
        [
            entry("reserve", namespace, "k-owned"),
            entry("release", namespace, "k-owned"),
            entry("reserve", namespace, "k-owned"),
            entry("release", namespace, "k-owned"),
            entry("reserve", namespace, "k-foreign"),
            entry("complete", namespace, "k-foreign"),
            entry("reserve", namespace, "k-foreign"),
        ],
    );
    for (_, recorded, _) in &journal {
        assert!(!recorded.contains(raw_id), "id verbatim in {recorded}");
    }
    client.close().await.unwrap();
}

#[tokio::test]
async fn a_users_aborted_call_is_released_under_its_hashed_namespace() {
    owner_and_non_owner(user("u-1"), &bucket("mcp", "u-1"), "u-1").await;
}

#[tokio::test]
async fn a_system_callers_aborted_call_is_released_under_its_hashed_namespace() {
    let system = SystemContext::for_service("svc").into_context();
    owner_and_non_owner(system, &bucket("mcp-system", "system:svc"), "svc").await;
}
