//! Every response the layer emits itself carries the framework error
//! envelope, so a generated client decodes a typed code instead of
//! "unrecognized error body" (cratestack#846).

#![cfg(test)]

use std::sync::Arc;

use axum::body::Body;
use axum::extract::Request;
use cratestack_codec_cbor::CborCodec;
use cratestack_core::{CratestackCodec, CratestackErrorResponse, RateLimitConfig};
use http::StatusCode;
use tower::{Layer as TowerLayer, Service};

use super::layer::RateLimitLayer;
use super::policy::StoreErrorPolicy;
use super::tests_support::{authed_request, content_type_and_body, ok_service};

/// The throttled 429 is the response a healthy limiter emits most often,
/// and it had the same opaque body. It must decode to a typed code — and
/// keep its `Retry-After`.
#[tokio::test]
async fn throttled_response_body_decodes_to_a_typed_code() {
    let store = Arc::new(super::store::InMemoryRateLimitStore::new());
    // Burst of 1, effectively no refill: the second request throttles.
    let layer = RateLimitLayer::new(store, RateLimitConfig::new(1, 0.001));
    let mut svc = layer.layer(ok_service());

    assert_eq!(
        svc.call(authed_request()).await.unwrap().status(),
        StatusCode::OK
    );
    let throttled = svc.call(authed_request()).await.unwrap();
    assert_eq!(throttled.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(
        throttled.headers().get(http::header::RETRY_AFTER).is_some(),
        "Retry-After must survive the switch to an encoded body"
    );

    let (content_type, body) = content_type_and_body(throttled).await;
    assert_eq!(content_type, "application/cbor");
    let decoded: CratestackErrorResponse = CborCodec
        .decode(&body)
        .expect("the 429 body must decode as the framework error envelope");
    assert_eq!(decoded.code, "TOO_MANY_REQUESTS");
    assert_eq!(decoded.message, "rate limit exceeded");
}

/// Key derivation stays fail-CLOSED (cratestack#416) — the policy knob
/// deliberately does not reach it, for the same reason the `OOM` case
/// stays closed: its inputs are caller-controlled.
#[tokio::test]
async fn key_derivation_failure_still_refuses_but_with_a_typed_body() {
    let store = Arc::new(super::store::InMemoryRateLimitStore::new());
    let layer = RateLimitLayer::new(store, RateLimitConfig::new(10, 1.0))
        .with_store_error_policy(StoreErrorPolicy::Allow);
    let mut svc = layer.layer(ok_service());

    // No Authorization header and no ConnectInfo: no verifiable identity.
    let response = svc
        .call(Request::builder().body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::PRECONDITION_FAILED,
        "StoreErrorPolicy::Allow must not soften the identity check"
    );
    let (_, body) = content_type_and_body(response).await;
    let decoded: CratestackErrorResponse = CborCodec
        .decode(&body)
        .expect("the refusal body must decode as the framework error envelope");
    assert_eq!(decoded.code, "PRECONDITION_FAILED");
}
