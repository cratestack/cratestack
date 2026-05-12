//! End-to-end tests: real HTTP requests through `RateLimitLayer` backed
//! by `RedisRateLimitStore` against a live Redis.
//!
//! Companion to `e2e.rs` (which covers `IdempotencyLayer`). These exist
//! to prove that the Redis store plugs into the middleware end-to-end
//! and that the response surface — status codes, `Retry-After`,
//! `X-RateLimit-*` headers — matches the contract the layer documents.
//! Tests skip cleanly when `CRATESTACK_REDIS_TEST_URL` is unset.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, Response, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::post;
use cratestack_axum::ratelimit::{RateLimitConfig, RateLimitLayer};
use cratestack_redis::RedisRateLimitStore;
use tower::util::ServiceExt;
use uuid::Uuid;

const TEST_AUTH: &str = "Bearer test-token";

fn store_or_skip(suffix: &str) -> Option<RedisRateLimitStore> {
    let url = std::env::var("CRATESTACK_REDIS_TEST_URL").ok()?;
    let prefix = format!(
        "cratestack:test:e2e-rl:{suffix}:{}",
        Uuid::new_v4().simple()
    );
    RedisRateLimitStore::open(url, prefix).ok()
}

fn build_router<H, Fut>(
    store: RedisRateLimitStore,
    config: RateLimitConfig,
    handler: H,
) -> Router
where
    H: Clone + Send + Sync + 'static + Fn() -> Fut,
    Fut: std::future::Future<Output = Response<Body>> + Send + 'static,
{
    let store = Arc::new(store);
    Router::new()
        .route("/transfer", post(move || handler.clone()()))
        .layer(RateLimitLayer::new(store, config))
}

async fn body_string(response: Response<Body>) -> (StatusCode, axum::http::HeaderMap, String) {
    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, 4 * 1024 * 1024).await.expect("read body");
    let text = String::from_utf8(bytes.to_vec()).expect("utf8 body");
    (parts.status, parts.headers, text)
}

fn post_request(auth: &str) -> Request<Body> {
    Request::post("/transfer")
        .header("authorization", auth)
        .header("content-type", "application/json")
        .body(Body::from(r#"{"amount":100}"#))
        .expect("request")
}

// -----------------------------------------------------------------------------
// Happy path: requests up to `burst` succeed and carry the X-RateLimit-* hints
// so clients can self-pace; the next request returns 429 with a Retry-After.
// -----------------------------------------------------------------------------

#[tokio::test]
async fn allows_up_to_burst_then_returns_429_with_retry_after() {
    let Some(store) = store_or_skip("burst") else {
        return;
    };
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = Arc::clone(&counter);
    // Refill slow enough that the burst is the only thing that runs.
    let config = RateLimitConfig::new(3, 0.001);
    let router = build_router(store, config, move || {
        let counter = Arc::clone(&counter_clone);
        async move {
            counter.fetch_add(1, Ordering::SeqCst);
            (StatusCode::CREATED, "ok").into_response()
        }
    });

    for i in 0..3 {
        let response = router
            .clone()
            .oneshot(post_request(TEST_AUTH))
            .await
            .expect("send");
        assert_eq!(response.status(), StatusCode::CREATED, "attempt {i}");
        // The layer must surface the bucket hints on every allowed
        // response — banks build client-side backoff on these.
        assert_eq!(
            response.headers().get("x-ratelimit-limit").map(|v| v.as_bytes()),
            Some(b"3".as_slice()),
            "X-RateLimit-Limit must reflect the configured burst",
        );
        assert!(
            response.headers().get("x-ratelimit-remaining").is_some(),
            "X-RateLimit-Remaining must accompany allowed responses",
        );
    }

    let throttled = router
        .clone()
        .oneshot(post_request(TEST_AUTH))
        .await
        .expect("throttled");
    let (status, headers, body) = body_string(throttled).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    let retry_after = headers
        .get(header::RETRY_AFTER)
        .expect("Retry-After must accompany 429");
    let parsed: u32 = retry_after
        .to_str()
        .expect("ASCII Retry-After")
        .parse()
        .expect("integer Retry-After");
    assert!(parsed >= 1, "Retry-After must be at least 1 second");
    assert!(
        body.contains("rate limit"),
        "response body should describe the throttle, got {body:?}",
    );
    assert_eq!(
        counter.load(Ordering::SeqCst),
        3,
        "handler must run exactly burst-many times; got {}",
        counter.load(Ordering::SeqCst),
    );
}

// -----------------------------------------------------------------------------
// Two distinct principals (different `Authorization` headers) must have
// independent buckets — exhausting one does not throttle the other.
// -----------------------------------------------------------------------------

#[tokio::test]
async fn distinct_principals_have_independent_buckets() {
    let Some(store) = store_or_skip("isolation") else {
        return;
    };
    let config = RateLimitConfig::new(1, 0.001);
    let router = build_router(store, config, || async {
        (StatusCode::CREATED, "ok").into_response()
    });

    // Alice consumes her one token.
    let alice_ok = router
        .clone()
        .oneshot(post_request("Bearer alice"))
        .await
        .expect("alice ok");
    assert_eq!(alice_ok.status(), StatusCode::CREATED);

    // Bob's bucket is untouched; his request must still succeed.
    let bob_ok = router
        .clone()
        .oneshot(post_request("Bearer bob"))
        .await
        .expect("bob ok");
    assert_eq!(
        bob_ok.status(),
        StatusCode::CREATED,
        "Bob must not see Alice's exhaustion",
    );

    // Now both are exhausted.
    let alice_throttled = router
        .clone()
        .oneshot(post_request("Bearer alice"))
        .await
        .expect("alice 2");
    assert_eq!(alice_throttled.status(), StatusCode::TOO_MANY_REQUESTS);
}

// -----------------------------------------------------------------------------
// Two replicas (two Router instances sharing the same Redis store) must enforce
// a single global bucket — the whole point of swapping the in-memory store for
// Redis. We simulate replicas by building two routers over the same store and
// alternating requests between them.
// -----------------------------------------------------------------------------

#[tokio::test]
async fn replicas_sharing_the_redis_store_enforce_a_single_global_bucket() {
    let Some(store) = store_or_skip("multi-replica") else {
        return;
    };
    let store = Arc::new(store);
    let config = RateLimitConfig::new(2, 0.001);

    let make_router = || {
        Router::new()
            .route(
                "/transfer",
                post(|| async { (StatusCode::CREATED, "ok").into_response() }),
            )
            .layer(RateLimitLayer::new(Arc::clone(&store) as Arc<_>, config))
    };
    let replica_a = make_router();
    let replica_b = make_router();

    // Two requests across two replicas exhausts the shared burst of 2.
    let r1 = replica_a
        .clone()
        .oneshot(post_request(TEST_AUTH))
        .await
        .expect("r1");
    let r2 = replica_b
        .clone()
        .oneshot(post_request(TEST_AUTH))
        .await
        .expect("r2");
    assert_eq!(r1.status(), StatusCode::CREATED);
    assert_eq!(r2.status(), StatusCode::CREATED);

    // The third request — on either replica — must observe the shared
    // bucket as empty. This is the property an in-memory store would
    // not provide: each replica would have its own bucket and grant a
    // third token.
    let r3 = replica_a
        .clone()
        .oneshot(post_request(TEST_AUTH))
        .await
        .expect("r3");
    assert_eq!(
        r3.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "shared Redis bucket must be empty after two grants across replicas",
    );
    let r4 = replica_b
        .clone()
        .oneshot(post_request(TEST_AUTH))
        .await
        .expect("r4");
    assert_eq!(r4.status(), StatusCode::TOO_MANY_REQUESTS);
}

// -----------------------------------------------------------------------------
// Refill grants new tokens after the wall-clock elapsed time, so a client that
// hits 429 and waits the suggested Retry-After gets through on the next try.
// -----------------------------------------------------------------------------

#[tokio::test]
async fn client_recovers_after_waiting_retry_after() {
    let Some(store) = store_or_skip("retry-after") else {
        return;
    };
    // 1 token burst, refill 100/sec — so 10ms is enough to refill.
    let config = RateLimitConfig::new(1, 100.0);
    let router = build_router(store, config, || async {
        (StatusCode::CREATED, "ok").into_response()
    });

    let first = router
        .clone()
        .oneshot(post_request(TEST_AUTH))
        .await
        .expect("first");
    assert_eq!(first.status(), StatusCode::CREATED);

    let throttled = router
        .clone()
        .oneshot(post_request(TEST_AUTH))
        .await
        .expect("throttled");
    assert_eq!(throttled.status(), StatusCode::TOO_MANY_REQUESTS);

    // Refill window is ~10ms; the layer ceilings Retry-After to a whole
    // second, but the bucket itself refills well before then, so we
    // only need to wait long enough for the script to observe new
    // tokens.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let recovered = router
        .clone()
        .oneshot(post_request(TEST_AUTH))
        .await
        .expect("recovered");
    assert_eq!(
        recovered.status(),
        StatusCode::CREATED,
        "client must recover once the refill lands",
    );
}

// -----------------------------------------------------------------------------
// Anonymous (no Authorization header) requests share a single bucket — the
// default key function maps them all to `"anonymous"`. Banks that want a
// per-IP bucket override the key function; this test pins the default.
// -----------------------------------------------------------------------------

// -----------------------------------------------------------------------------
// Randomized HTTP-level property: across random (burst, request_count, key)
// triples, the layer must serve exactly `burst` 201s and 429 the overflow.
// Seeded by CRATESTACK_TEST_SEED for reproducibility.
// -----------------------------------------------------------------------------

fn test_seed() -> u64 {
    std::env::var("CRATESTACK_TEST_SEED")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0x9E37_79B9_7F4A_7C15)
}

struct XorShift64(u64);
impl XorShift64 {
    fn new(seed: u64) -> Self {
        Self(if seed == 0 { 0xDEAD_BEEF_CAFE_BABE } else { seed })
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn next_range(&mut self, lo: u32, hi: u32) -> u32 {
        lo + ((self.next_u64() as u32) % (hi - lo + 1))
    }
}

#[tokio::test]
async fn randomized_http_burst_then_429_holds_for_arbitrary_configs() {
    let Some(base_store) = store_or_skip("rand-e2e") else {
        return;
    };
    let seed = test_seed();
    let mut rng = XorShift64::new(seed);
    // Run a few independent random configurations, each with its own
    // bucket key (a unique Authorization header) so they don't share
    // state across iterations.
    for iteration in 0..4 {
        let burst = rng.next_range(1, 6);
        let extra = rng.next_range(3, 6);
        let auth = format!("Bearer rand-{}-{}", iteration, rng.next_u64());

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);
        let store = base_store.clone();
        let config = RateLimitConfig::new(burst, 0.0001); // negligible refill
        let router = build_router(store, config, move || {
            let counter = Arc::clone(&counter_clone);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                (StatusCode::CREATED, "ok").into_response()
            }
        });

        let mut ok = 0u32;
        let mut throttled = 0u32;
        for attempt in 0..(burst + extra) {
            let response = router
                .clone()
                .oneshot(post_request(&auth))
                .await
                .unwrap_or_else(|err| {
                    panic!(
                        "seed={seed:#x} iter={iteration} burst={burst} attempt={attempt}: {err:?}",
                    )
                });
            match response.status() {
                StatusCode::CREATED => ok += 1,
                StatusCode::TOO_MANY_REQUESTS => {
                    throttled += 1;
                    // Every 429 must carry Retry-After ≥ 1.
                    let retry = response
                        .headers()
                        .get(header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|s| s.parse::<u32>().ok())
                        .unwrap_or_else(|| {
                            panic!(
                                "seed={seed:#x} iter={iteration} attempt={attempt}: 429 missing Retry-After",
                            )
                        });
                    assert!(
                        retry >= 1,
                        "seed={seed:#x} iter={iteration} attempt={attempt}: Retry-After={retry} must be >= 1",
                    );
                }
                other => panic!(
                    "seed={seed:#x} iter={iteration} attempt={attempt}: unexpected status {other}",
                ),
            }
        }
        assert_eq!(
            ok, burst,
            "seed={seed:#x} iter={iteration} burst={burst}: should allow exactly burst",
        );
        assert_eq!(
            throttled, extra,
            "seed={seed:#x} iter={iteration} burst={burst} extra={extra}: should throttle the overflow",
        );
        assert_eq!(
            counter.load(Ordering::SeqCst) as u32,
            burst,
            "seed={seed:#x} iter={iteration}: handler must run exactly burst times",
        );
    }
}

#[tokio::test]
async fn anonymous_requests_share_one_bucket_under_the_default_key_fn() {
    let Some(store) = store_or_skip("anonymous") else {
        return;
    };
    let config = RateLimitConfig::new(2, 0.001);
    let router = build_router(store, config, || async {
        (StatusCode::CREATED, "ok").into_response()
    });

    let unauth_req = || {
        Request::post("/transfer")
            .header("content-type", "application/json")
            .body(Body::from(r#"{}"#))
            .expect("req")
    };

    let r1 = router.clone().oneshot(unauth_req()).await.expect("r1");
    let r2 = router.clone().oneshot(unauth_req()).await.expect("r2");
    assert_eq!(r1.status(), StatusCode::CREATED);
    assert_eq!(r2.status(), StatusCode::CREATED);
    let r3 = router.clone().oneshot(unauth_req()).await.expect("r3");
    assert_eq!(
        r3.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "the third anonymous request must hit the shared bucket's limit",
    );
}
