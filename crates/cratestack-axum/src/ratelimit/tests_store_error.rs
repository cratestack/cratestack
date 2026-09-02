//! What the layer does when the *store* fails, how long it is willing to
//! wait to find out, and what the body of every response it emits itself
//! looks like on the wire (cratestack#846, and its security review).

#![cfg(test)]

use std::sync::Arc;
use std::time::{Duration, Instant};

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
use super::tests_support::{
    CapturingLayer, RefusingStore, SlowStore, UnreachableStore, content_type_and_body,
};

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

type CapturedEvents = Arc<std::sync::Mutex<Vec<(tracing::Level, String)>>>;

/// `#[tokio::test]` drives a current-thread runtime, so the thread-local
/// default subscriber this guard installs stays in effect across the
/// `.await` points below — no separate runtime needed. The throttles the
/// layer logs through are per-layer (see `super::policy`), so a fresh
/// layer per test means these assertions are order-independent.
fn capture_logs() -> (tracing::subscriber::DefaultGuard, CapturedEvents) {
    let capture = CapturingLayer::default();
    let events = capture.events.clone();
    let guard = tracing::subscriber::set_default(tracing_subscriber::registry().with(capture));
    (guard, events)
}

// ---------------------------------------------------------------------
// The security-review finding: `Allow` is class-conditional.
// ---------------------------------------------------------------------

/// A store that cannot be reached is the one case the default serves
/// through: nothing a caller sent caused it, no caller can fix it, and it
/// self-heals when the socket is replaced.
#[tokio::test]
async fn an_unreachable_store_is_served_through_under_the_default_policy() {
    let (_guard, events) = capture_logs();
    let layer = RateLimitLayer::new(Arc::new(UnreachableStore), RateLimitConfig::new(10, 1.0));
    let mut svc = layer.layer(ok_service());
    let status = svc.call(authed_request()).await.unwrap().status();
    let captured = events.lock().unwrap().clone();

    assert_eq!(
        status,
        StatusCode::OK,
        "a transport-class store failure must degrade to unlimited, not to a 500 on every \
         rate-limited route"
    );
    assert!(
        captured.iter().any(|(level, msg)| *level == tracing::Level::WARN
            && msg.contains("rate limit store error")
            && msg.contains("served_unthrottled=true")),
        "the WARN must record that the limiter stopped limiting, got: {captured:?}"
    );
}

/// **The blocker.** The default key function hashes an *unvalidated*
/// `Authorization` header, so an unauthenticated caller mints one Redis
/// key per request; drive that to `maxmemory` and every write fails with
/// `OOM`. If `Allow` served through *that*, anyone could switch the
/// limiter off globally — including for buckets already exhausted. An
/// `OOM` is reachable, deterministic and caller-induced, so it must be
/// refused under `Allow` exactly as under `Deny`.
#[tokio::test]
async fn a_reachable_but_refusing_store_is_refused_even_under_the_default_policy() {
    let (_guard, events) = capture_logs();
    let layer = RateLimitLayer::new(Arc::new(RefusingStore), RateLimitConfig::new(10, 1.0));
    let mut svc = layer.layer(ok_service());
    let response = svc.call(authed_request()).await.unwrap();
    let captured = events.lock().unwrap().clone();

    assert_eq!(
        response.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "an OOM (or any reachable-but-refusing store) must NOT open the gate: it is \
         caller-inducible, so serving through it is a global limiter bypass"
    );
    let (content_type, body) = content_type_and_body(response).await;
    assert_eq!(content_type, "application/cbor");
    let decoded: CratestackErrorResponse = CborCodec
        .decode(&body)
        .expect("the refusal body must decode as the framework error envelope");
    assert_eq!(decoded.code, "INTERNAL_ERROR");
    assert_eq!(
        decoded.message, "internal error",
        "the driver's OOM text stays operator-only"
    );
    assert!(
        captured
            .iter()
            .any(|(_, msg)| msg.contains("served_unthrottled=false")),
        "the WARN must record that the request was refused, got: {captured:?}"
    );
}

/// `Deny` is unconditional — even the transport-class case refuses.
#[tokio::test]
async fn deny_refuses_an_unreachable_store_with_a_decodable_typed_body() {
    let layer = RateLimitLayer::new(Arc::new(UnreachableStore), RateLimitConfig::new(10, 1.0))
        .with_store_error_policy(StoreErrorPolicy::Deny);
    let mut svc = layer.layer(ok_service());
    let response = svc.call(authed_request()).await.unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let (_, body) = content_type_and_body(response).await;
    let decoded: CratestackErrorResponse = CborCodec.decode(&body).expect(
        "the refusal body must decode as the framework error envelope — decoding it as \
         anything else is the 'unrecognized error body' bug from cratestack#846",
    );
    assert_eq!(decoded.code, "UNAVAILABLE");
}

// ---------------------------------------------------------------------
// The second blocker: "degrade to unlimited" must mean "promptly".
// ---------------------------------------------------------------------

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

// ---------------------------------------------------------------------
// Typed bodies on the layer's own responses.
// ---------------------------------------------------------------------

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
