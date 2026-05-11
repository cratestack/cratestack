//! End-to-end test for the `IdempotencyLayer` against a real Postgres-backed
//! store.
//!
//! Confirms that:
//! - same key + same body replays the stored response with the
//!   `Idempotency-Replayed: true` marker;
//! - same key + different body returns 422 (`idempotency_key_conflict`);
//! - GET requests bypass the layer entirely.

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_schema;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::sqlx::query;
use cratestack::{AuthProvider, CoolCodec, CoolContext, CoolError, RequestContext, Value};
use cratestack_axum::idempotency::IdempotencyLayer;
use cratestack_codec_json::JsonCodec;
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceBuilder;
use tower::util::ServiceExt;

include_schema!("tests/fixtures/banking_idempotency.cstack");

async fn serial_guard() -> tokio::sync::MutexGuard<'static, ()> {
    static M: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    M.lock().await
}

async fn connect_or_skip() -> Option<cratestack::sqlx::PgPool> {
    let database_url = std::env::var("CRATESTACK_TEST_DATABASE_URL").ok()?;
    // The concurrency test runs two requests in parallel and each is
    // doing reserve + handler + complete; bump the pool above 2 so the
    // middleware doesn't deadlock on connection acquisition.
    PgPoolOptions::new()
        .max_connections(8)
        .connect(&database_url)
        .await
        .ok()
}

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS cratestack_idempotency, vouchers")
        .execute(pool)
        .await
        .expect("drop");
    query(
        "CREATE TABLE vouchers (
            id BIGINT PRIMARY KEY,
            code TEXT NOT NULL,
            amount BIGINT NOT NULL
        )",
    )
    .execute(pool)
    .await
    .expect("create voucher");
}

#[derive(Clone)]
struct StaticAuth;

impl AuthProvider for StaticAuth {
    type Error = CoolError;
    fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CoolContext, Self::Error>> + Send {
        core::future::ready(Ok(CoolContext::authenticated([(
            "id".to_owned(),
            Value::Int(1),
        )])))
    }
}

fn build_router(pool: cratestack::sqlx::PgPool) -> cratestack::axum::Router {
    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let base = cratestack_schema::axum::model_router(cool, JsonCodec, StaticAuth);
    let store = Arc::new(cratestack::SqlxIdempotencyStore::new(pool));
    base.layer(ServiceBuilder::new().layer(IdempotencyLayer::new(store, Duration::from_secs(60))))
}

async fn body_string(resp: cratestack::axum::http::Response<Body>) -> String {
    let (parts, body) = resp.into_parts();
    let bytes = to_bytes(body, 1024 * 1024).await.expect("read body");
    let text = std::str::from_utf8(&bytes).expect("utf8 body").to_owned();
    drop(parts);
    text
}

#[tokio::test]
async fn same_key_same_body_replays_response_with_marker_header() {
    let _guard = serial_guard().await;
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    reset_schema(&pool).await;

    // Ensure the idempotency table exists. The layer is best-effort about
    // store init in Phase 1; banks run migrations themselves.
    let store = cratestack::SqlxIdempotencyStore::new(pool.clone());
    store.ensure_schema().await.expect("ensure schema");

    let router = build_router(pool.clone());
    let body = r#"{"id":1,"code":"V-1","amount":500}"#;

    let first = router
        .clone()
        .oneshot(
            Request::post("/vouchers")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .header("idempotency-key", "txn-001")
                .body(Body::from(body))
                .expect("req"),
        )
        .await
        .expect("first");
    assert_eq!(first.status(), StatusCode::CREATED);
    let first_body = body_string(first).await;

    // The DB now has the row; if the layer didn't intercept the replay it
    // would 409 on the duplicate primary key. The replay should short-circuit
    // that and return the cached response instead.
    let second = router
        .clone()
        .oneshot(
            Request::post("/vouchers")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .header("idempotency-key", "txn-001")
                .body(Body::from(body))
                .expect("req"),
        )
        .await
        .expect("second");

    assert_eq!(second.status(), StatusCode::CREATED);
    let replayed = second
        .headers()
        .get("idempotency-replayed")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    assert_eq!(replayed.as_deref(), Some("true"));
    let second_body = body_string(second).await;
    assert_eq!(first_body, second_body, "replay body must match original");

    // And no second row was inserted.
    let count: (i64,) = cratestack::sqlx::query_as("SELECT COUNT(*)::BIGINT FROM vouchers")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(count.0, 1, "replay must not create a duplicate row");
}

#[tokio::test]
async fn same_key_different_body_returns_idempotency_conflict() {
    let _guard = serial_guard().await;
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    reset_schema(&pool).await;
    cratestack::SqlxIdempotencyStore::new(pool.clone())
        .ensure_schema()
        .await
        .expect("ensure schema");

    let router = build_router(pool);

    let first_body = r#"{"id":2,"code":"V-2","amount":100}"#;
    let second_body = r#"{"id":2,"code":"V-2","amount":999}"#;

    let first = router
        .clone()
        .oneshot(
            Request::post("/vouchers")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .header("idempotency-key", "txn-conflict")
                .body(Body::from(first_body))
                .expect("req"),
        )
        .await
        .expect("first");
    assert_eq!(first.status(), StatusCode::CREATED);

    let second = router
        .clone()
        .oneshot(
            Request::post("/vouchers")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .header("idempotency-key", "txn-conflict")
                .body(Body::from(second_body))
                .expect("req"),
        )
        .await
        .expect("second");
    assert_eq!(second.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn concurrent_requests_with_same_key_execute_handler_exactly_once() {
    use cratestack::axum::Router;
    use cratestack::axum::response::IntoResponse;
    use cratestack::axum::routing::post;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let _guard = serial_guard().await;
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    cratestack::SqlxIdempotencyStore::new(pool.clone())
        .ensure_schema()
        .await
        .expect("ensure schema");
    cratestack::sqlx::query("DELETE FROM cratestack_idempotency")
        .execute(&pool)
        .await
        .expect("drain");

    // Use a tiny custom router with a deliberately slow handler so the
    // race window stays open long enough for both requests to reach
    // `reserve_or_fetch` before either completes. A schema-generated
    // route finishes too fast to reliably exercise the contended path.
    let invocations: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    let invocations_clone = Arc::clone(&invocations);
    let handler = move || {
        let counter = Arc::clone(&invocations_clone);
        async move {
            counter.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(200)).await;
            (StatusCode::CREATED, "{\"ok\":true}").into_response()
        }
    };
    let store = Arc::new(cratestack::SqlxIdempotencyStore::new(pool.clone()));
    let router: Router = Router::new()
        .route("/transfer", post(handler))
        .layer(IdempotencyLayer::new(store, Duration::from_secs(60)));

    let body = r#"{"from":1,"to":2,"amount":100}"#;
    let send = |router: Router| async move {
        router
            .oneshot(
                Request::post("/transfer")
                    .header("content-type", "application/json")
                    .header("idempotency-key", "transfer-001")
                    .body(Body::from(body))
                    .expect("req"),
            )
            .await
            .expect("send")
    };

    let a = tokio::spawn(send(router.clone()));
    let b = tokio::spawn(send(router.clone()));
    let (ra, rb) = tokio::join!(a, b);
    let ra = ra.expect("task a");
    let rb = rb.expect("task b");

    // Handler must fire exactly once even though two concurrent
    // requests arrived — that's the banking-grade duplicate-execution
    // protection the reservation pattern is here to provide.
    assert_eq!(
        invocations.load(Ordering::SeqCst),
        1,
        "handler must run exactly once across the two parallel requests",
    );

    // One request gets the live response (CREATED, possibly with the
    // `Idempotency-Replayed: true` marker if it observed the completed
    // record), the other either replays the completion or gets a 409
    // `InFlight`. Both are valid; what matters is that no second
    // execution leaks through.
    let statuses = [ra.status(), rb.status()];
    let acceptable = statuses
        .iter()
        .all(|s| *s == StatusCode::CREATED || *s == StatusCode::CONFLICT);
    assert!(
        acceptable,
        "expected each response to be either 201 or 409; got {statuses:?}",
    );
}

#[tokio::test]
async fn expired_reservation_can_be_replaced_on_reuse() {
    let _guard = serial_guard().await;
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    reset_schema(&pool).await;
    cratestack::SqlxIdempotencyStore::new(pool.clone())
        .ensure_schema()
        .await
        .expect("ensure schema");

    // Seed an idempotency row whose TTL has already expired. The
    // previous best-effort `ON CONFLICT DO NOTHING` path would leave
    // this row in place and silently re-run the handler on every
    // duplicate until the GC sweep caught up, so the post-expiry
    // duplicate would never establish a fresh idempotency window.
    cratestack::sqlx::query(
        "INSERT INTO cratestack_idempotency (
            principal_fingerprint, key, request_hash,
            response_status, response_content_type, response_body,
            created_at, expires_at
        ) VALUES (
            'fingerprint-static', 'recycled', $1,
            201, 'application/json', $2,
            NOW() - INTERVAL '2 hours',
            NOW() - INTERVAL '1 hour'
        )",
    )
    .bind(vec![0u8; 32])
    .bind(br#"{"stale":true}"#.to_vec())
    .execute(&pool)
    .await
    .expect("seed expired row");

    // Override the principal fingerprint to deterministically match the
    // seeded row regardless of the request's Authorization header. Banks
    // running real auth would derive this from their auth provider.
    let store = Arc::new(cratestack::SqlxIdempotencyStore::new(pool.clone()));
    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let base = cratestack_schema::axum::model_router(cool, JsonCodec, StaticAuth);
    let router = base.layer(
        IdempotencyLayer::new(store, Duration::from_secs(60))
            .with_principal_fingerprint(|_| "fingerprint-static".to_owned()),
    );

    let body = r#"{"id":10,"code":"V-RECYCLED","amount":1}"#;
    let response = router
        .oneshot(
            Request::post("/vouchers")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .header("idempotency-key", "recycled")
                .body(Body::from(body))
                .expect("req"),
        )
        .await
        .expect("send");

    // Expired row was taken over, fresh handler ran, fresh response
    // returned — and crucially the response is the LIVE one, not a
    // replay of the stale `{"stale":true}` payload.
    assert_eq!(response.status(), StatusCode::CREATED);
    assert!(
        response.headers().get("idempotency-replayed").is_none(),
        "expired-row replacement must run the handler, not replay the stale response",
    );
    let body_text = body_string(response).await;
    assert!(
        !body_text.contains("stale"),
        "response body should be the freshly created voucher, not the stale cached payload: {body_text}",
    );

    // And the new row replaced the stale one — there's still exactly
    // one entry for this (principal, key), but its response body and
    // expires_at reflect the live execution.
    let (response_body, expires_at): (Option<Vec<u8>>, chrono::DateTime<chrono::Utc>) =
        cratestack::sqlx::query_as(
            "SELECT response_body, expires_at FROM cratestack_idempotency
             WHERE principal_fingerprint = 'fingerprint-static' AND key = 'recycled'",
        )
        .fetch_one(&pool)
        .await
        .expect("read");
    let body_bytes = response_body.expect("completed row must have a body");
    let body_str = std::str::from_utf8(&body_bytes).expect("utf8");
    assert!(
        !body_str.contains("stale"),
        "the persisted row must be the new completion, not the stale one: {body_str}",
    );
    assert!(
        expires_at > chrono::Utc::now(),
        "the new row's TTL must be in the future, not the stale '1 hour ago' value",
    );
}

#[tokio::test]
async fn get_requests_bypass_idempotency_layer_entirely() {
    let _guard = serial_guard().await;
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    reset_schema(&pool).await;
    cratestack::SqlxIdempotencyStore::new(pool.clone())
        .ensure_schema()
        .await
        .expect("ensure schema");

    // Seed a row to read.
    query("INSERT INTO vouchers VALUES (3, 'V-3', 1)")
        .execute(&pool)
        .await
        .expect("seed");
    let router = build_router(pool);

    let response = router
        .oneshot(
            Request::get("/vouchers/3")
                .header("accept", JsonCodec::CONTENT_TYPE)
                .header("idempotency-key", "should-be-ignored")
                .body(Body::empty())
                .expect("req"),
        )
        .await
        .expect("get");

    assert_eq!(response.status(), StatusCode::OK);
    let replayed = response.headers().get("idempotency-replayed");
    assert!(
        replayed.is_none(),
        "GET responses must not be tagged as idempotent replays",
    );
}
