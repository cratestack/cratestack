#![cfg(test)]

use std::sync::Arc;

use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use http::StatusCode;
use tower::{Layer as TowerLayer, Service};

use super::config::{_bucket_capacity_for, RateLimitConfig, RateLimitDecision};
use super::layer::RateLimitLayer;
use super::store::{InMemoryRateLimitStore, RateLimitStore};

#[tokio::test]
async fn allows_up_to_burst_then_throttles() {
    let store = InMemoryRateLimitStore::new();
    let config = RateLimitConfig::new(3, 0.001); // very slow refill
    for i in 0..3 {
        let decision = store.consume("k", config).await.unwrap();
        assert!(
            matches!(decision, RateLimitDecision::Allowed { .. }),
            "attempt {i} should be allowed: {decision:?}",
        );
    }
    let decision = store.consume("k", config).await.unwrap();
    assert!(matches!(decision, RateLimitDecision::Throttled { .. }));
}

#[tokio::test]
async fn refill_grants_more_tokens_after_wait() {
    let store = InMemoryRateLimitStore::new();
    let config = RateLimitConfig::new(2, 1000.0); // refills instantly
    // exhaust
    store.consume("k", config).await.unwrap();
    store.consume("k", config).await.unwrap();
    // sleep a hair, then expect refill to allow another
    tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
    let decision = store.consume("k", config).await.unwrap();
    assert!(matches!(decision, RateLimitDecision::Allowed { .. }));
}

#[tokio::test]
async fn per_key_isolation_does_not_leak_between_principals() {
    let store = InMemoryRateLimitStore::new();
    let config = RateLimitConfig::new(1, 0.001);
    let a = store.consume("alice", config).await.unwrap();
    let b = store.consume("bob", config).await.unwrap();
    assert!(matches!(a, RateLimitDecision::Allowed { .. }));
    assert!(matches!(b, RateLimitDecision::Allowed { .. }));
    let a_throttled = store.consume("alice", config).await.unwrap();
    assert!(matches!(a_throttled, RateLimitDecision::Throttled { .. }));
}

#[test]
fn capacity_helper_passes_burst() {
    assert_eq!(_bucket_capacity_for(RateLimitConfig::new(7, 1.0)), 7);
}

/// cratestack#474: `with_should_rate_limit_fn` returning `false` must skip
/// the store-consume check entirely — the exemption applies at the
/// `RateLimitLayer`/`RateLimitService` level, independent of any
/// descriptor plumbing built on top of it (`build_rpc_ops_filter`,
/// `build_rest_ops_filter`). This is the one test that exercises the
/// actual behavioral change (the bypass branch in `RateLimitService::call`)
/// without a DB, an RPC router, or reqwest — unlike the DB-gated
/// integration test in `cratestack-pg`.
#[tokio::test]
async fn should_rate_limit_fn_returning_false_bypasses_the_store_entirely() {
    let store = Arc::new(InMemoryRateLimitStore::new());
    // Burst of 1: a control request through the default filter would be
    // throttled on the very next call.
    let config = RateLimitConfig::new(1, 0.001);
    let layer = RateLimitLayer::new(store, config).with_should_rate_limit_fn(|_req| false);
    let inner = tower::service_fn(|_req: Request| async {
        Ok::<_, std::convert::Infallible>(Response::new(Body::from("ok")))
    });
    let mut svc = layer.layer(inner);

    for i in 0..5 {
        let req = Request::builder().body(Body::empty()).unwrap();
        let status = svc.call(req).await.unwrap().status();
        assert_eq!(
            status,
            StatusCode::OK,
            "request {i} should succeed: an exempt filter must bypass the \
             burst limit entirely, not just raise it"
        );
    }
}

/// Control case for the test above: the *default* filter (no
/// `with_should_rate_limit_fn` call) still throttles once the burst is
/// exhausted, proving the bypass is opt-in behavior, not a change to the
/// default fail-closed posture.
#[tokio::test]
async fn default_filter_still_throttles_without_should_rate_limit_fn() {
    let store = Arc::new(InMemoryRateLimitStore::new());
    let config = RateLimitConfig::new(1, 0.001);
    let layer = RateLimitLayer::new(store, config);
    let inner = tower::service_fn(|_req: Request| async {
        Ok::<_, std::convert::Infallible>(Response::new(Body::from("ok")))
    });
    let mut svc = layer.layer(inner);

    // ConnectInfo stands in for a real socket peer (cratestack#416 made the
    // default key fn refuse requests with neither Authorization nor
    // ConnectInfo — this test is about should_rate_limit_fn's default, not
    // that refusal, so it needs a verifiable identity to reach the store).
    let peer: std::net::SocketAddr = "192.0.2.10:1".parse().unwrap();
    let mut first = Request::builder().body(Body::empty()).unwrap();
    first
        .extensions_mut()
        .insert(axum::extract::ConnectInfo(peer));
    assert_eq!(svc.call(first).await.unwrap().status(), StatusCode::OK);

    let mut second = Request::builder().body(Body::empty()).unwrap();
    second
        .extensions_mut()
        .insert(axum::extract::ConnectInfo(peer));
    assert_eq!(
        svc.call(second).await.unwrap().status(),
        StatusCode::TOO_MANY_REQUESTS,
        "without an explicit should_rate_limit_fn, the default fails closed \
         (always rate-limits) once the burst is exhausted"
    );
}
