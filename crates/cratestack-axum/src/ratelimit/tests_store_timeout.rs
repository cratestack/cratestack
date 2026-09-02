//! The store lookup is bounded in wall-clock (cratestack#846 security
//! review, blocker 2).
//!
//! Before the budget existed, "degrade to unlimited" silently meant
//! "hang for the driver's unbounded reconnect cycle, then allow" —
//! measured at 9.46s per attempt against a real outage, 18.92s once the
//! retry doubled it. That is worse for the caller than the refusal it
//! replaced, and is itself a denial-of-service lever.

#![cfg(test)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use cratestack_codec_cbor::CborCodec;
use cratestack_core::{CratestackCodec, CratestackErrorResponse, RateLimitConfig};
use http::StatusCode;
use tower::{Layer as TowerLayer, Service};

use super::layer::RateLimitLayer;
use super::policy::StoreErrorPolicy;
use super::tests_support::{
    SlowStore, authed_request, content_type_and_body, ok_service,
};

const BUDGET: Duration = Duration::from_millis(150);

/// Before the budget existed, a store that never answered made the
/// request wait out the driver's unbounded reconnect cycle — measured at
/// 9.46s, doubled to 18.92s by the retry. Serving unthrottled after
/// nineteen seconds is worse for the caller than the refusal it replaced.
#[tokio::test]
async fn a_hanging_store_is_served_through_within_the_budget_not_after_it() {
    let layer = RateLimitLayer::new(
        Arc::new(SlowStore {
            delay: Duration::from_secs(30),
        }),
        RateLimitConfig::new(10, 1.0),
    )
    .with_store_timeout(BUDGET);
    let mut svc = layer.layer(ok_service());

    let started = Instant::now();
    let status = svc.call(authed_request()).await.unwrap().status();
    let elapsed = started.elapsed();

    assert_eq!(status, StatusCode::OK, "a timeout is transport-class");
    assert!(
        elapsed < BUDGET * 2,
        "the caller must not wait out the store: budget {BUDGET:?}, waited {elapsed:?}"
    );
}

/// The same ceiling applies when the policy is to refuse — `Deny` must
/// not mean "hang, then refuse".
#[tokio::test]
async fn a_hanging_store_is_refused_within_the_budget_under_deny() {
    let layer = RateLimitLayer::new(
        Arc::new(SlowStore {
            delay: Duration::from_secs(30),
        }),
        RateLimitConfig::new(10, 1.0),
    )
    .with_store_timeout(BUDGET)
    .with_store_error_policy(StoreErrorPolicy::Deny);
    let mut svc = layer.layer(ok_service());

    let started = Instant::now();
    let response = svc.call(authed_request()).await.unwrap();
    let elapsed = started.elapsed();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        elapsed < BUDGET * 2,
        "budget {BUDGET:?}, waited {elapsed:?}"
    );
    let (_, body) = content_type_and_body(response).await;
    let decoded: CratestackErrorResponse = CborCodec.decode(&body).expect("typed envelope");
    assert_eq!(decoded.code, "UNAVAILABLE");
}

/// A store that answers inside the budget must be unaffected by it — the
/// ceiling must not turn a healthy lookup into a timeout.
#[tokio::test]
async fn a_prompt_store_is_not_disturbed_by_the_budget() {
    let layer = RateLimitLayer::new(
        Arc::new(SlowStore {
            delay: Duration::from_millis(1),
        }),
        RateLimitConfig::new(10, 1.0),
    )
    .with_store_timeout(BUDGET);
    let mut svc = layer.layer(ok_service());

    let response = svc.call(authed_request()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers().get("x-ratelimit-limit").is_some(),
        "a real Allowed decision still carries its budget hints"
    );
}

