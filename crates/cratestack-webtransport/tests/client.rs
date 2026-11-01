//! The public native client (feature `client`) against the real server:
//! `Init` with headers, one stream per subscription, items, the clean end,
//! refusals at `Init` and at `Request`, a terminal error, the pinned
//! certificate, and the cancel reaching the dispatcher.

mod support;

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::time::Duration;

use cratestack_webtransport::{ClientFailure, ServerTrust, WebTransportClient, WebTransportLimits};
use serde_json::json;

fn headers(token: &str) -> BTreeMap<String, String> {
    [("authorization".to_owned(), format!("Bearer {token}"))].into()
}

fn pinned(running: &support::Running) -> ServerTrust {
    ServerTrust::CertificateHashes(vec![running.hash.clone().try_into().unwrap()])
}

async fn client(running: &support::Running, token: &str) -> WebTransportClient {
    let url = running.url.parse().unwrap();
    WebTransportClient::connect(&url, &pinned(running), headers(token))
        .await
        .expect("the session opens")
}

fn refused_code(failure: ClientFailure) -> String {
    match failure {
        ClientFailure::Refused(body) => body.code,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn subscriptions_share_a_session_and_end_cleanly() {
    let running = support::start(WebTransportLimits::default());
    let client = client(&running, "7").await;
    let mut items = client
        .subscribe("test.items", json!({ "n": 3 }))
        .await
        .unwrap();
    let mut who = client.subscribe("test.whoami", json!(null)).await.unwrap();

    assert_eq!(who.next().await.unwrap().unwrap(), json!("7"));
    let mut seen = Vec::new();
    while let Some(item) = items.next().await {
        seen.push(item.unwrap());
    }
    assert_eq!(seen, [json!({"i": 0}), json!({"i": 1}), json!({"i": 2})]);
}

#[tokio::test]
async fn refusals_and_terminal_errors_are_refused() {
    let running = support::start(WebTransportLimits::default());
    let url = running.url.parse().unwrap();
    let refused = WebTransportClient::connect(&url, &pinned(&running), headers("bad")).await;
    assert_eq!(refused_code(refused.err().unwrap()), "unauthenticated");

    let client = client(&running, "7").await;
    let refused = client.subscribe("test.refuse", json!(null)).await;
    assert_eq!(refused_code(refused.err().unwrap()), "permission_denied");

    let mut failing = client
        .subscribe("test.fail_mid", json!(null))
        .await
        .unwrap();
    assert_eq!(failing.next().await.unwrap().unwrap(), json!("first"));
    assert_eq!(
        refused_code(failing.next().await.unwrap().unwrap_err()),
        "unavailable"
    );
    assert!(failing.next().await.is_none());
}

#[tokio::test]
async fn dropping_a_subscription_cancels_it() {
    let running = support::start(WebTransportLimits::default());
    let client = client(&running, "7").await;
    let mut forever = client.subscribe("test.forever", json!(null)).await.unwrap();
    assert!(forever.next().await.unwrap().is_ok());
    drop(forever);
    tokio::time::timeout(Duration::from_secs(5), async {
        while !running.dispatcher.dropped.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the cancel reaches the dispatcher");
}

#[tokio::test]
async fn an_unpinned_or_unrooted_certificate_is_unreachable() {
    let running = support::start(WebTransportLimits::default());
    let url = running.url.parse().unwrap();
    let wrong = ServerTrust::CertificateHashes(vec![[0u8; 32]]);
    let failure = WebTransportClient::connect(&url, &wrong, headers("7")).await;
    assert!(
        matches!(failure, Err(ClientFailure::Unreachable(_))),
        "{failure:?}"
    );

    let roots = WebTransportClient::connect(&url, &ServerTrust::NativeRoots, headers("7")).await;
    assert!(
        matches!(roots, Err(ClientFailure::Unreachable(_))),
        "self-signed is not a root"
    );
}
