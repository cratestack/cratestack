//! End-to-end test for the `RateLimitLayer`.
//!
//! No PG needed — we build a minimal axum router with a single GET handler
//! and confirm the layer:
//! - allows the first burst, emitting `X-RateLimit-*` headers;
//! - returns 429 with `Retry-After` once the bucket is empty;
//! - keys are isolated per-principal (the `Authorization` header is the
//!   default key fn input).

use cratestack::axum::Router;
use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::axum::response::IntoResponse;
use cratestack::axum::routing::get;
use cratestack_axum::ratelimit::{InMemoryRateLimitStore, RateLimitConfig, RateLimitLayer};
use std::sync::Arc;
use tower::util::ServiceExt;

async fn handler() -> impl IntoResponse {
    "ok"
}

fn build_router(config: RateLimitConfig) -> Router {
    let store = Arc::new(InMemoryRateLimitStore::new());
    Router::new()
        .route("/ping", get(handler))
        .layer(RateLimitLayer::new(store, config))
}

fn auth_header(value: &'static str) -> (&'static str, &'static str) {
    ("authorization", value)
}

#[tokio::test]
async fn first_request_passes_and_carries_rate_limit_headers() {
    let router = build_router(RateLimitConfig::new(3, 0.001));
    let (k, v) = auth_header("Bearer alice");

    let response = router
        .oneshot(
            Request::get("/ping")
                .header(k, v)
                .body(Body::empty())
                .expect("req"),
        )
        .await
        .expect("send");

    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers();
    assert_eq!(
        headers
            .get("x-ratelimit-limit")
            .and_then(|v| v.to_str().ok()),
        Some("3"),
    );
    assert!(
        headers.get("x-ratelimit-remaining").is_some(),
        "X-RateLimit-Remaining should be present on allowed responses",
    );
}

#[tokio::test]
async fn exhausting_the_bucket_yields_429_with_retry_after() {
    let router = build_router(RateLimitConfig::new(2, 0.001));
    let (k, v) = auth_header("Bearer bob");

    for _ in 0..2 {
        let response = router
            .clone()
            .oneshot(
                Request::get("/ping")
                    .header(k, v)
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("send");
        assert_eq!(response.status(), StatusCode::OK);
    }

    let blocked = router
        .oneshot(
            Request::get("/ping")
                .header(k, v)
                .body(Body::empty())
                .expect("req"),
        )
        .await
        .expect("send");
    assert_eq!(blocked.status(), StatusCode::TOO_MANY_REQUESTS);
    let retry_after = blocked
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u32>().ok());
    assert!(
        retry_after.is_some(),
        "Retry-After header should be present and numeric on 429",
    );
}

#[tokio::test]
async fn buckets_are_isolated_per_authorization_header() {
    let router = build_router(RateLimitConfig::new(1, 0.001));

    let (k, alice) = auth_header("Bearer alice");
    let (_, bob) = auth_header("Bearer bob");

    let alice_first = router
        .clone()
        .oneshot(
            Request::get("/ping")
                .header(k, alice)
                .body(Body::empty())
                .expect("req"),
        )
        .await
        .expect("send");
    assert_eq!(alice_first.status(), StatusCode::OK);

    let alice_second = router
        .clone()
        .oneshot(
            Request::get("/ping")
                .header(k, alice)
                .body(Body::empty())
                .expect("req"),
        )
        .await
        .expect("send");
    assert_eq!(alice_second.status(), StatusCode::TOO_MANY_REQUESTS);

    // Bob has his own bucket; one request should pass even after Alice is
    // throttled.
    let bob_first = router
        .oneshot(
            Request::get("/ping")
                .header(k, bob)
                .body(Body::empty())
                .expect("req"),
        )
        .await
        .expect("send");
    assert_eq!(bob_first.status(), StatusCode::OK);
}
