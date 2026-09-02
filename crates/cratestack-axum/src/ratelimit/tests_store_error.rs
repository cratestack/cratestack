//! What the layer does when the *store* fails, and what the body of
//! every response the layer emits itself looks like on the wire
//! (cratestack#846).

#![cfg(test)]

use std::sync::Arc;

use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use cratestack_codec_cbor::CborCodec;
use cratestack_core::{CratestackCodec, CratestackErrorResponse, RateLimitConfig};
use http::StatusCode;
use tower::{Layer as TowerLayer, Service};
use tracing_subscriber::layer::SubscriberExt;

use super::layer::RateLimitLayer;
use super::policy::StoreErrorPolicy;
use super::tests_support::{CapturingLayer, FailingStore, content_type_and_body};

async fn ok_handler(_req: Request) -> Result<Response, std::convert::Infallible> {
    Ok(Response::new(Body::from("ok")))
}

type OkService = tower::util::ServiceFn<fn(Request) -> OkFuture>;
type OkFuture = std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<Response, std::convert::Infallible>> + Send>,
>;

/// Spelled with an explicit fn-pointer type rather than an `impl Trait`
/// return: `RateLimitService`'s `Service` impl requires `S::Future: Send`,
/// which an opaque `impl Service` return type does not carry.
fn ok_service() -> OkService {
    fn make(req: Request) -> OkFuture {
        Box::pin(ok_handler(req))
    }
    tower::service_fn(make as fn(Request) -> OkFuture)
}

/// A verifiable caller identity, so the request reaches the (failing)
/// store rather than being refused by the default key fn itself
/// (cratestack#416).
fn authed_request() -> Request {
    Request::builder()
        .header("authorization", "Bearer test")
        .body(Body::empty())
        .unwrap()
}

/// `#[tokio::test]` drives a current-thread runtime, so the thread-local
/// default subscriber this guard installs stays in effect across the
/// `.await` points below — no separate runtime needed.
fn capture_logs() -> (tracing::subscriber::DefaultGuard, Arc<std::sync::Mutex<Vec<(tracing::Level, String)>>>)
{
    let capture = CapturingLayer::default();
    let events = capture.events.clone();
    let guard = tracing::subscriber::set_default(tracing_subscriber::registry().with(capture));
    (guard, events)
}

/// cratestack#846: the default is now fail-OPEN. A store outage is not
/// caller-controlled and no caller can fix it, so refusing would convert a
/// limiter hiccup into a total outage of every rate-limited route. The
/// request must reach the inner service — and the failure must still be
/// loud, because this WARN is the only trace that the limiter is not
/// actually limiting.
#[tokio::test]
async fn store_error_under_the_default_policy_reaches_the_inner_service_and_warns() {
    let (_guard, events) = capture_logs();
    let layer = RateLimitLayer::new(Arc::new(FailingStore), RateLimitConfig::new(10, 1.0));
    let mut svc = layer.layer(ok_service());
    let status = svc.call(authed_request()).await.unwrap().status();
    let captured = events.lock().unwrap().clone();

    assert_eq!(
        status,
        StatusCode::OK,
        "with StoreErrorPolicy::Allow (the default) a store outage must degrade to unlimited, \
         not to a 500 on every rate-limited route"
    );
    assert!(
        captured
            .iter()
            .any(|(level, msg)| *level == tracing::Level::WARN
                && msg.contains("rate limit store error")
                && msg.contains("redis rate limit: connection refused")),
        "expected a WARN carrying the underlying store error text, got: {captured:?}"
    );
    assert!(
        captured
            .iter()
            .any(|(_, msg)| msg.contains("policy=Allow")),
        "the WARN must record which policy was in effect, got: {captured:?}"
    );
}

/// The opposite half of the knob: deployments using the limiter as a
/// security control opt into refusing, and get the framework's typed
/// error envelope rather than the old opaque `text/plain` body.
#[tokio::test]
async fn store_error_under_deny_refuses_with_a_decodable_typed_body() {
    let layer = RateLimitLayer::new(Arc::new(FailingStore), RateLimitConfig::new(10, 1.0))
        .with_store_error_policy(StoreErrorPolicy::Deny);
    let mut svc = layer.layer(ok_service());
    let response = svc.call(authed_request()).await.unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let (content_type, body) = content_type_and_body(response).await;
    assert_eq!(content_type, "application/cbor");
    let decoded: CratestackErrorResponse = CborCodec.decode(&body).expect(
        "the refusal body must decode as the framework error envelope — decoding it as \
         anything else is the 'unrecognized error body' bug from cratestack#846",
    );
    assert_eq!(decoded.code, "INTERNAL_ERROR");
    assert_eq!(
        decoded.message, "internal error",
        "5xx detail stays operator-only, same redaction the handler path applies"
    );
}

/// The throttled 429 is the response a healthy limiter emits most often,
/// and it had the same opaque body. It must decode to a typed code too —
/// and keep its `Retry-After`.
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
/// deliberately does not reach it — but its refusal now carries the same
/// typed envelope instead of a bare `text/plain` message.
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
        "StoreErrorPolicy::Allow must not soften the identity check: those inputs ARE \
         caller-controlled, which is why cratestack#416 made them fail closed"
    );
    let (_, body) = content_type_and_body(response).await;
    let decoded: CratestackErrorResponse = CborCodec
        .decode(&body)
        .expect("the refusal body must decode as the framework error envelope");
    assert_eq!(decoded.code, "PRECONDITION_FAILED");
}
