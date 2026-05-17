//! End-to-end tests: real HTTP requests through `IdempotencyLayer`
//! backed by `RedisIdempotencyStore` against a live Redis.
//!
//! These mirror `crates/cratestack/tests/banking_idempotency.rs`'s
//! HTTP-level scenarios but stand on their own — they don't depend on
//! the cratestack schema/codec machinery, just axum + tower + the layer.
//! Tests skip cleanly when `CRATESTACK_REDIS_TEST_URL` is unset.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderValue, Request, Response, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use cratestack_axum::idempotency::IdempotencyLayer;
use cratestack_redis::RedisIdempotencyStore;
use tower::util::ServiceExt;
use uuid::Uuid;

/// The default `IdempotencyLayer` derives its principal fingerprint
/// from the `Authorization` header — easy to pin in tests by sending
/// the same header on every request, or by overriding the extractor.
const TEST_AUTH: &str = "Bearer test-token";

fn store_or_skip(suffix: &str) -> Option<RedisIdempotencyStore> {
    let url = std::env::var("CRATESTACK_REDIS_TEST_URL").ok()?;
    let prefix = format!("cratestack:test:e2e:{suffix}:{}", Uuid::new_v4().simple());
    RedisIdempotencyStore::open(url, prefix).ok()
}

fn build_router<H, Fut>(store: RedisIdempotencyStore, handler: H) -> Router
where
    H: Clone + Send + Sync + 'static + Fn() -> Fut,
    Fut: std::future::Future<Output = Response<Body>> + Send + 'static,
{
    let store = Arc::new(store);
    Router::new()
        .route("/transfer", post(move || handler.clone()()))
        .route(
            "/transfer/{id}",
            get(|| async { (StatusCode::OK, "GET ok") }),
        )
        .layer(IdempotencyLayer::new(store, Duration::from_secs(60)))
}

async fn body_string(response: Response<Body>) -> (StatusCode, axum::http::HeaderMap, String) {
    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, 4 * 1024 * 1024).await.expect("read body");
    let text = String::from_utf8(bytes.to_vec()).expect("utf8 body");
    (parts.status, parts.headers, text)
}

fn post_request(body: &'static str, idempotency_key: &str) -> Request<Body> {
    Request::post("/transfer")
        .header("authorization", TEST_AUTH)
        .header("content-type", "application/json")
        .header("idempotency-key", idempotency_key)
        .body(Body::from(body))
        .expect("request")
}

// -----------------------------------------------------------------------------
// Happy path: same key + same body replays with `Idempotency-Replayed: true`.
// -----------------------------------------------------------------------------

#[tokio::test]
async fn same_key_same_body_replays_response_with_marker_header() {
    let Some(store) = store_or_skip("replay") else {
        return;
    };
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = Arc::clone(&counter);
    let router = build_router(store, move || {
        let counter = Arc::clone(&counter_clone);
        async move {
            counter.fetch_add(1, Ordering::SeqCst);
            (StatusCode::CREATED, r#"{"transfer_id":"abc"}"#).into_response()
        }
    });

    let first = router
        .clone()
        .oneshot(post_request(r#"{"amount":100}"#, "txn-001"))
        .await
        .expect("first send");
    let (status_a, _headers_a, body_a) = body_string(first).await;
    assert_eq!(status_a, StatusCode::CREATED);

    let second = router
        .clone()
        .oneshot(post_request(r#"{"amount":100}"#, "txn-001"))
        .await
        .expect("second send");
    let (status_b, headers_b, body_b) = body_string(second).await;
    assert_eq!(status_b, StatusCode::CREATED);
    assert_eq!(body_a, body_b, "replay body must match original");
    assert_eq!(
        headers_b.get("idempotency-replayed"),
        Some(&HeaderValue::from_static("true")),
        "replay must be marked",
    );
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "handler must run exactly once across the two identical requests",
    );
}

// -----------------------------------------------------------------------------
// Conflict path: same key + different body → 422 idempotency_key_conflict.
// -----------------------------------------------------------------------------

#[tokio::test]
async fn same_key_different_body_returns_422_conflict() {
    let Some(store) = store_or_skip("conflict") else {
        return;
    };
    let router = build_router(store, || async {
        (StatusCode::CREATED, "ok").into_response()
    });

    let first = router
        .clone()
        .oneshot(post_request(r#"{"amount":100}"#, "txn-conflict"))
        .await
        .expect("first");
    assert_eq!(first.status(), StatusCode::CREATED);

    let second = router
        .clone()
        .oneshot(post_request(r#"{"amount":999}"#, "txn-conflict"))
        .await
        .expect("second");
    let (status, headers, _body) = body_string(second).await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "different body under same key must conflict",
    );
    assert!(
        headers.get("idempotency-replayed").is_none(),
        "conflict response must not carry the replay marker",
    );
}

// -----------------------------------------------------------------------------
// Different idempotency keys must each run the handler.
// -----------------------------------------------------------------------------

#[tokio::test]
async fn different_keys_each_run_the_handler() {
    let Some(store) = store_or_skip("distinct-keys") else {
        return;
    };
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = Arc::clone(&counter);
    let router = build_router(store, move || {
        let counter = Arc::clone(&counter_clone);
        async move {
            counter.fetch_add(1, Ordering::SeqCst);
            (StatusCode::CREATED, "ok").into_response()
        }
    });

    for n in 0..5 {
        let key = format!("txn-{n}");
        let response = router
            .clone()
            .oneshot(post_request(r#"{"amount":1}"#, &key))
            .await
            .expect("send");
        assert_eq!(response.status(), StatusCode::CREATED);
    }
    assert_eq!(
        counter.load(Ordering::SeqCst),
        5,
        "each distinct key must invoke the handler",
    );
}

// -----------------------------------------------------------------------------
// Replay must preserve the response headers the handler emitted —
// banking flows depend on Location, ETag, Cache-Control, etc. being
// stable across retries.
// -----------------------------------------------------------------------------

#[tokio::test]
async fn replay_preserves_response_headers_emitted_by_the_handler() {
    let Some(store) = store_or_skip("headers") else {
        return;
    };
    let router = build_router(store, || async {
        let mut response = (StatusCode::CREATED, r#"{"transfer_id":"abc"}"#).into_response();
        let headers = response.headers_mut();
        headers.insert("location", HeaderValue::from_static("/transfers/abc"));
        headers.insert("etag", HeaderValue::from_static("\"v7\""));
        headers.insert("cache-control", HeaderValue::from_static("no-store"));
        response
    });

    let first = router
        .clone()
        .oneshot(post_request(r#"{"amount":100}"#, "txn-headers"))
        .await
        .expect("first");
    let (_, headers_a, _) = body_string(first).await;
    let location_a = headers_a
        .get("location")
        .cloned()
        .expect("Location on first");

    let second = router
        .clone()
        .oneshot(post_request(r#"{"amount":100}"#, "txn-headers"))
        .await
        .expect("second");
    let (status, headers_b, _) = body_string(second).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        headers_b.get("idempotency-replayed").map(|v| v.as_bytes()),
        Some(b"true".as_slice()),
    );
    assert_eq!(
        headers_b.get("location"),
        Some(&location_a),
        "replay must preserve Location",
    );
    assert_eq!(
        headers_b.get("etag").map(|v| v.as_bytes()),
        Some(b"\"v7\"".as_slice()),
        "replay must preserve ETag",
    );
    assert_eq!(
        headers_b.get("cache-control").map(|v| v.as_bytes()),
        Some(b"no-store".as_slice()),
        "replay must preserve Cache-Control",
    );
}

// -----------------------------------------------------------------------------
// GET requests bypass the layer entirely.
// -----------------------------------------------------------------------------

#[tokio::test]
async fn get_requests_bypass_the_layer_entirely() {
    let Some(store) = store_or_skip("get-bypass") else {
        return;
    };
    let router = build_router(store, || async {
        (StatusCode::CREATED, "should-not-be-called").into_response()
    });

    let response = router
        .clone()
        .oneshot(
            Request::get("/transfer/42")
                .header("authorization", TEST_AUTH)
                .header("idempotency-key", "ignored-on-get")
                .body(Body::empty())
                .expect("req"),
        )
        .await
        .expect("send");
    let (status, headers, _) = body_string(response).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        headers.get("idempotency-replayed").is_none(),
        "GET responses must not be tagged as idempotent replays",
    );
}

// -----------------------------------------------------------------------------
// Concurrent POSTs with the same key + body run the handler exactly once.
// -----------------------------------------------------------------------------

#[tokio::test]
async fn concurrent_posts_run_handler_exactly_once() {
    let Some(store) = store_or_skip("concurrent-http") else {
        return;
    };
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = Arc::clone(&counter);
    // Slow the handler enough that both requests reach `reserve_or_fetch`
    // before either completes — without this the second request would
    // already see Replay and we wouldn't be testing the InFlight path.
    let router = build_router(store, move || {
        let counter = Arc::clone(&counter_clone);
        async move {
            counter.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(200)).await;
            (StatusCode::CREATED, r#"{"ok":true}"#).into_response()
        }
    });

    let send = |router: Router| async move {
        router
            .oneshot(post_request(r#"{"amount":1}"#, "txn-concurrent-http"))
            .await
            .expect("send")
    };
    let a = tokio::spawn(send(router.clone()));
    let b = tokio::spawn(send(router.clone()));
    let (ra, rb) = tokio::join!(a, b);
    let ra = ra.expect("task a");
    let rb = rb.expect("task b");
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "handler must run exactly once across two parallel requests",
    );
    // One request gets the live response (201), the other either gets
    // a replay (also 201, with the marker header) or a 409 InFlight if
    // it arrived during the reservation window. Both are valid per the
    // trait contract — what matters is no double-execution.
    let statuses = [ra.status(), rb.status()];
    assert!(
        statuses
            .iter()
            .all(|s| *s == StatusCode::CREATED || *s == StatusCode::CONFLICT),
        "expected each response to be either 201 or 409; got {statuses:?}",
    );
}

// -----------------------------------------------------------------------------
// Two distinct principals (different `Authorization` headers) sharing the
// same idempotency key must NOT see each other's responses.
// -----------------------------------------------------------------------------

#[tokio::test]
async fn distinct_principals_under_same_key_get_isolated_responses() {
    let Some(store) = store_or_skip("principal-iso") else {
        return;
    };
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = Arc::clone(&counter);
    let router = build_router(store, move || {
        let counter = Arc::clone(&counter_clone);
        async move {
            let n = counter.fetch_add(1, Ordering::SeqCst);
            (StatusCode::CREATED, format!(r#"{{"invocation":{n}}}"#)).into_response()
        }
    });

    let req = |auth: &'static str| {
        Request::post("/transfer")
            .header("authorization", auth)
            .header("content-type", "application/json")
            .header("idempotency-key", "txn-shared-key")
            .body(Body::from(r#"{"amount":1}"#))
            .expect("req")
    };

    let alice = router
        .clone()
        .oneshot(req("Bearer alice"))
        .await
        .expect("alice");
    let bob = router
        .clone()
        .oneshot(req("Bearer bob"))
        .await
        .expect("bob");
    let (_, _, body_a) = body_string(alice).await;
    let (_, _, body_b) = body_string(bob).await;
    assert_ne!(
        body_a, body_b,
        "different principals under the same key must run the handler twice and see distinct responses",
    );
    assert_eq!(
        counter.load(Ordering::SeqCst),
        2,
        "each principal must invoke the handler independently",
    );
}
