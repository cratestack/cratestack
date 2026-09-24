//! Resources over Streamable HTTP (cratestack#1040 on top of cratestack#1039).
//!
//! Phase 4 made the caller per request: the guard runs the application's
//! `AuthProvider` and every handler resolves the context it built. These
//! tests pin that a resource read takes *that* caller for both halves of
//! its work, whose rows it sees and whose rate-limit bucket it is charged
//! to, and that the two list methods sit behind the same guard.
//!
//! The table is `support::owned::OwnedResources`, whose rows depend on the
//! reader, so a read that ran under a stored, fixed or anonymous context
//! answers differently from one that ran as the token's caller.

mod support;

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use cratestack_core::{RateLimitConfig, RateLimitStore, SystemContext};
use cratestack_mcp::{OpExecutor, StdioServer};
use serde_json::{Value, json};
use support::client::Client;
use support::http_app::{RESOURCE, builder, mount, post, resource_read, rpc, send, served, token};
use support::owned::OwnedResources;
use support::stores::CountingLimiter;

fn ids(page: &Value) -> Vec<u64> {
    let text = page["contents"][0]["text"]
        .as_str()
        .expect("one text block");
    let body: Value = serde_json::from_str(text).unwrap();
    body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_u64().unwrap())
        .collect()
}

#[tokio::test]
async fn two_tokens_reading_one_collection_each_see_only_their_own_rows() {
    let table = OwnedResources::default();
    let app = served(&table);
    let (a, b) = (token("u-a"), token("u-b"));
    let collection = "cratestack://blog/posts";

    let seen_by_a = send(&app, resource_read("/mcp", Some(&a), collection)).await;
    let seen_by_b = send(&app, resource_read("/mcp", Some(&b), collection)).await;
    assert_eq!(ids(&seen_by_a.result()), [1, 3, 5]);
    assert_eq!(ids(&seen_by_b.result()), [2, 4, 6]);

    // The same record, two callers: its owner reads it, the other gets the
    // not-found a missing row gets.
    let record = "cratestack://blog/posts/2";
    let owned = send(&app, resource_read("/mcp", Some(&b), record)).await;
    let text = owned.result()["contents"][0]["text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap()["owner"],
        "u-b"
    );
    let hidden = send(&app, resource_read("/mcp", Some(&a), record))
        .await
        .json();
    assert_eq!(hidden["error"]["message"], "resource not found", "{hidden}");

    let expected = ["u-a", "u-b", "u-b", "u-a"].map(|id| Some(id.to_owned()));
    assert_eq!(table.readers(), expected, "each read ran as its own token");
}

#[tokio::test]
async fn each_readers_rate_limit_bucket_is_its_own() {
    let table = OwnedResources::default();
    let limiter = Arc::new(CountingLimiter::new(100));
    let executor = OpExecutor::new(None, Duration::ZERO).with_rate_limit(
        limiter.clone() as Arc<dyn RateLimitStore>,
        RateLimitConfig::new(100, 0.0),
    );
    let server = builder(&table, RESOURCE)
        .with_executor(executor)
        .build()
        .unwrap();
    let app = mount(&server);

    for (caller, uri) in [
        ("u-a", "cratestack://blog/posts"),
        ("u-b", "cratestack://blog/posts/2"),
    ] {
        let reply = send(&app, resource_read("/mcp", Some(&token(caller)), uri)).await;
        reply.result();
    }

    let keys = limiter.keys.lock().unwrap().clone();
    assert_eq!(keys, ["mcp:u-a", "mcp:u-b"], "one bucket per caller");
}

/// The cratestack#1039 split holds for reads too: a token whose `id` claims
/// a service's system id is charged to a user bucket, never to the
/// service's, so it cannot spend (or be throttled by) the service's budget.
#[tokio::test]
async fn a_system_reader_and_a_token_claiming_its_id_are_charged_apart() {
    let table = OwnedResources::default();
    let limiter = Arc::new(CountingLimiter::new(100));
    let executor = OpExecutor::new(None, Duration::ZERO).with_rate_limit(
        limiter.clone() as Arc<dyn RateLimitStore>,
        RateLimitConfig::new(100, 0.0),
    );
    let uri = json!({ "uri": "cratestack://blog/posts" });

    let system = SystemContext::for_service("svc").into_context();
    let stdio = StdioServer::new(table.clone(), system).unwrap();
    let mut client = Client::start(stdio.with_executor(executor.clone()));
    let read = client.request("resources/read", uri).await;
    assert!(read.get("result").is_some(), "{read}");
    client.close().await.unwrap();

    let server = builder(&table, RESOURCE)
        .with_executor(executor)
        .build()
        .unwrap();
    let impostor = token("system:svc");
    let collection = "cratestack://blog/posts";
    send(
        &mount(&server),
        resource_read("/mcp", Some(&impostor), collection),
    )
    .await
    .result();

    let keys = limiter.keys.lock().unwrap().clone();
    assert_eq!(keys, ["mcp-system:system:svc", "mcp:system:svc"]);
}

#[tokio::test]
async fn the_resource_lists_sit_behind_the_guard() {
    let app = served(&OwnedResources::default());
    for method in ["resources/list", "resources/templates/list"] {
        let body = rpc(method, json!({}));
        let request = |token: Option<&str>| {
            post("/mcp", token, &body)
                .body(Body::from(body.to_string()))
                .unwrap()
        };

        let refused = send(&app, request(None)).await;
        assert_eq!(refused.status, 401, "{method}: {}", refused.text);
        assert!(refused.header("www-authenticate").starts_with("Bearer "));

        let listed = send(&app, request(Some(&token("u-a")))).await.result();
        let key = if method == "resources/list" {
            "resources"
        } else {
            "resourceTemplates"
        };
        assert!(
            !listed[key].as_array().unwrap().is_empty(),
            "{method}: {listed}"
        );
    }
}
