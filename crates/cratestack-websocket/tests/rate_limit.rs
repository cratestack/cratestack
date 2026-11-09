//! Subscription opens are charged per `Request` (design §6.7), against a
//! scripted limiter so the binding is tested without axum's rate limiter.

mod rate_limit_support;

use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::Ordering;

use cratestack_websocket::{WebSocketConfig, WebSocketLimits};
use rate_limit_support::{Budget, open_socket, recv, request, serve};

#[tokio::test]
async fn each_request_is_charged_and_a_throttled_one_is_refused_before_ready() {
    let budget = Budget::allowing(1);
    let url = serve(WebSocketConfig::new().rate_limit(budget.clone()), true).await;
    let mut socket = open_socket(&url).await;

    request(&mut socket, 1, "count", 2).await;
    assert!(recv(&mut socket).await.ready.is_some());
    assert_eq!(recv(&mut socket).await.next, Some(serde_json::json!(1)));
    assert_eq!(recv(&mut socket).await.next, Some(serde_json::json!(2)));
    assert!(recv(&mut socket).await.end.is_some());

    request(&mut socket, 2, "count", 2).await;
    let refused = recv(&mut socket).await;
    assert_eq!(refused.id, 2);
    assert!(refused.ready.is_none());
    let err = refused.err.expect("an err frame");
    assert_eq!(err.code, "resource_exhausted");
    assert_eq!(
        err.details,
        Some(serde_json::json!({ "retry_after_secs": 5 }))
    );

    // The socket stayed up, and the next frame is a fresh charge.
    budget.allow.store(1, Ordering::SeqCst);
    request(&mut socket, 3, "count", 1).await;
    let reply = recv(&mut socket).await;
    assert!(reply.id == 3 && reply.ready.is_some(), "{reply:?}");
    assert_eq!(budget.seen.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn the_limiter_sees_op_principal_and_peer() {
    let budget = Budget::allowing(1);
    let url = serve(WebSocketConfig::new().rate_limit(budget.clone()), true).await;
    let mut socket = open_socket(&url).await;
    request(&mut socket, 1, "count", 0).await;
    assert!(recv(&mut socket).await.ready.is_some());
    let seen = budget.seen.lock().unwrap().clone();
    let peer = Some(IpAddr::V4(Ipv4Addr::LOCALHOST));
    assert_eq!(seen, vec![("count".to_owned(), Some("1".to_owned()), peer)]);
}

#[tokio::test]
async fn without_connect_info_the_peer_is_none() {
    let budget = Budget::allowing(1);
    let url = serve(WebSocketConfig::new().rate_limit(budget.clone()), false).await;
    let mut socket = open_socket(&url).await;
    request(&mut socket, 1, "count", 0).await;
    assert!(recv(&mut socket).await.ready.is_some());
    assert_eq!(budget.seen.lock().unwrap()[0].2, None);
}

#[tokio::test]
async fn a_throttled_request_frees_its_slot() {
    let budget = Budget::allowing(0);
    let limits = WebSocketLimits {
        subscriptions_per_socket: 1,
        ..WebSocketLimits::default()
    };
    let config = WebSocketConfig::new()
        .limits(limits)
        .rate_limit(budget.clone());
    let url = serve(config, true).await;
    let mut socket = open_socket(&url).await;
    request(&mut socket, 1, "forever", 0).await;
    let first = recv(&mut socket).await;
    assert_eq!(first.err.expect("throttled").code, "resource_exhausted");

    budget.allow.store(1, Ordering::SeqCst);
    request(&mut socket, 2, "forever", 0).await;
    let second = recv(&mut socket).await;
    assert!(second.id == 2 && second.ready.is_some(), "{second:?}");
}

#[tokio::test]
async fn a_limiter_error_reaches_the_client() {
    let budget = Budget::allowing(1);
    *budget.fail.lock().unwrap() = true;
    let url = serve(WebSocketConfig::new().rate_limit(budget), false).await;
    let mut socket = open_socket(&url).await;
    request(&mut socket, 1, "count", 1).await;
    let refused = recv(&mut socket).await;
    assert_eq!(refused.err.expect("refused").code, "failed_precondition");
}
