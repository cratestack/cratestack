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
use cratestack::include_server_schema;
use cratestack::sqlx::query;
use cratestack::{AuthProvider, CoolCodec, CoolContext, CoolError, RequestContext, Value};
use cratestack_axum::idempotency::IdempotencyLayer;
use cratestack_codec_json::JsonCodec;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tower::ServiceBuilder;
use tower::util::ServiceExt;

include_server_schema!("tests/fixtures/banking_idempotency.cstack", db = Postgres);

mod support;

use support::pg;

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
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

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
        .fetch_one(pool)
        .await
        .expect("count");
    assert_eq!(count.0, 1, "replay must not create a duplicate row");
}

#[tokio::test]
async fn same_key_different_body_returns_idempotency_conflict() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    cratestack::SqlxIdempotencyStore::new(pool.clone())
        .ensure_schema()
        .await
        .expect("ensure schema");

    let router = build_router(pool.clone());

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

    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    // Drop the idempotency table outright so its schema matches the
    // current shape — a stale row left over from a previous test-binary
    // run could otherwise lack `reservation_id`, and `ensure_schema`'s
    // `CREATE TABLE IF NOT EXISTS` is a no-op against an existing
    // table.
    cratestack::sqlx::query("DROP TABLE IF EXISTS cratestack_idempotency")
        .execute(pool)
        .await
        .expect("drop idempotency");
    cratestack::SqlxIdempotencyStore::new(pool.clone())
        .ensure_schema()
        .await
        .expect("ensure schema");

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
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
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
            principal_fingerprint, key, request_hash, reservation_id,
            response_status, response_headers, response_body,
            created_at, expires_at
        ) VALUES (
            'fingerprint-static', 'recycled', $1, $2,
            201, $3, $4,
            NOW() - INTERVAL '2 hours',
            NOW() - INTERVAL '1 hour'
        )",
    )
    .bind(vec![0u8; 32])
    .bind(uuid::Uuid::new_v4())
    // Empty header blob — the seeded row's content-type info no
    // longer lives in a dedicated column; the replay path tolerates
    // an empty blob by emitting just the status + body.
    .bind(Vec::<u8>::new())
    .bind(br#"{"stale":true}"#.to_vec())
    .execute(pool)
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
        .fetch_one(pool)
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
async fn stale_handler_after_ttl_overrun_cannot_overwrite_newer_reservation() {
    use cratestack_axum::idempotency::{IdempotencyStore, ReservationOutcome};

    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    let store = cratestack::SqlxIdempotencyStore::new(pool.clone());
    store.ensure_schema().await.expect("ensure schema");

    let principal = "stale-fp";
    let key = "transfer-stale";
    let hash = [7u8; 32];
    let far_future = SystemTime::now() + Duration::from_secs(3600);

    // First caller — original handler — claims the reservation. We hang
    // onto its token to simulate the handler still running when the TTL
    // expires.
    let token_original = match store
        .reserve_or_fetch(principal, key, hash, far_future)
        .await
        .expect("first reservation")
    {
        ReservationOutcome::Reserved { token } => token,
        other => panic!("expected fresh reservation, got {other:?}"),
    };

    // Simulate the TTL elapsing while the original handler is still
    // running. Banks have seen this in practice when an upstream RPC
    // hangs longer than the idempotency window.
    cratestack::sqlx::query(
        "UPDATE cratestack_idempotency
         SET expires_at = NOW() - INTERVAL '1 second'
         WHERE principal_fingerprint = $1 AND key = $2",
    )
    .bind(principal)
    .bind(key)
    .execute(pool)
    .await
    .expect("expire row");

    // Retry arrives, sees the expired row, reclaims it with a fresh
    // token. The new token must differ from the original — otherwise
    // the guard collapses to the same row identity and we're back to
    // the pre-fix behaviour.
    let token_retry = match store
        .reserve_or_fetch(principal, key, hash, far_future)
        .await
        .expect("retry reservation")
    {
        ReservationOutcome::Reserved { token } => token,
        other => panic!("expected reclaim of expired row, got {other:?}"),
    };
    assert_ne!(
        token_retry, token_original,
        "reclaim must rotate the reservation token, otherwise stale handlers can still poison the row",
    );

    // Retry handler completes — this is the response that real callers
    // should observe on subsequent replays.
    store
        .complete(
            principal,
            key,
            token_retry,
            201,
            // No header replay needed for this token-guard test; the
            // headers blob is exercised separately via the HTTP layer.
            &[],
            br#"{"owner":"retry"}"#,
        )
        .await
        .expect("retry completion");

    // Now the original handler finally finishes (it was hung the whole
    // time). It tries to write its own response. With the token guard
    // in place this must be a silent no-op: the row's reservation_id
    // no longer matches.
    store
        .complete(
            principal,
            key,
            token_original,
            500,
            &[],
            br#"{"owner":"stale"}"#,
        )
        .await
        .expect("stale completion call must not surface an error");

    // The retry's response survived. If the stale completion had won,
    // the persisted body would say "stale".
    let body: Vec<u8> = cratestack::sqlx::query_scalar(
        "SELECT response_body FROM cratestack_idempotency
         WHERE principal_fingerprint = $1 AND key = $2",
    )
    .bind(principal)
    .bind(key)
    .fetch_one(pool)
    .await
    .expect("read body");
    let body_str = std::str::from_utf8(&body).expect("utf8");
    assert!(
        body_str.contains("retry"),
        "the retry's completion must remain — got: {body_str}",
    );
    assert!(
        !body_str.contains("stale"),
        "the stale handler must not be able to overwrite the newer reservation's body",
    );

    // The same guard applies to release: a stale release must not
    // delete the newer reservation's row either.
    store
        .release(principal, key, token_original)
        .await
        .expect("stale release call must not surface an error");
    let row_count: (i64,) = cratestack::sqlx::query_as(
        "SELECT COUNT(*)::BIGINT FROM cratestack_idempotency
         WHERE principal_fingerprint = $1 AND key = $2",
    )
    .bind(principal)
    .bind(key)
    .fetch_one(pool)
    .await
    .expect("count");
    assert_eq!(
        row_count.0, 1,
        "the retry's row must still be present after the stale release",
    );
}

#[tokio::test]
async fn replay_preserves_response_headers_emitted_by_the_handler() {
    use cratestack::axum::Router;
    use cratestack::axum::response::IntoResponse;
    use cratestack::axum::routing::post;

    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    cratestack::sqlx::query("DROP TABLE IF EXISTS cratestack_idempotency")
        .execute(pool)
        .await
        .expect("drop");
    cratestack::SqlxIdempotencyStore::new(pool.clone())
        .ensure_schema()
        .await
        .expect("ensure schema");

    // Build a tiny route whose handler returns 201 with the kind of
    // headers banking flows actually emit — Location for the created
    // resource, ETag for optimistic locking, Cache-Control to forbid
    // intermediate caching. Pre-fix the replay path only restored
    // `Content-Type`, so a retry would see a different response
    // shape from the original — exactly the bug we're fixing.
    let handler = move || async move {
        let mut response = (StatusCode::CREATED, "{\"transfer_id\":\"abc\"}").into_response();
        let headers = response.headers_mut();
        headers.insert(
            "location",
            cratestack::axum::http::HeaderValue::from_static("/transfers/abc"),
        );
        headers.insert(
            "etag",
            cratestack::axum::http::HeaderValue::from_static("\"v7\""),
        );
        headers.insert(
            "cache-control",
            cratestack::axum::http::HeaderValue::from_static("no-store"),
        );
        response
    };
    let store = Arc::new(cratestack::SqlxIdempotencyStore::new(pool.clone()));
    let router: Router = Router::new()
        .route("/transfers", post(handler))
        .layer(IdempotencyLayer::new(store, Duration::from_secs(60)));

    let body = r#"{"amount":100}"#;
    let send = |router: Router| async move {
        router
            .oneshot(
                Request::post("/transfers")
                    .header("content-type", "application/json")
                    .header("idempotency-key", "replay-headers-001")
                    .body(Body::from(body))
                    .expect("req"),
            )
            .await
            .expect("send")
    };

    let first = send(router.clone()).await;
    assert_eq!(first.status(), StatusCode::CREATED);
    let first_location = first
        .headers()
        .get("location")
        .expect("first response must carry Location")
        .clone();
    let first_etag = first
        .headers()
        .get("etag")
        .expect("first response must carry ETag")
        .clone();

    let second = send(router.clone()).await;
    assert_eq!(second.status(), StatusCode::CREATED);
    assert_eq!(
        second
            .headers()
            .get("idempotency-replayed")
            .map(|v| v.as_bytes()),
        Some(b"true".as_slice()),
    );
    assert_eq!(
        second.headers().get("location"),
        Some(&first_location),
        "replay must preserve Location so clients dereference the same resource",
    );
    assert_eq!(
        second.headers().get("etag"),
        Some(&first_etag),
        "replay must preserve ETag so optimistic-lock validators still match",
    );
    assert_eq!(
        second.headers().get("cache-control").map(|v| v.as_bytes()),
        Some(b"no-store".as_slice()),
        "replay must preserve Cache-Control directives the handler set",
    );
}

#[tokio::test]
async fn same_key_with_different_query_string_does_not_replay() {
    // Pre-fix the middleware hashed only `Uri::path`, so
    // `POST /vouchers?dry_run=true` and `POST /vouchers?dry_run=false`
    // collided under the same key — the second call would silently
    // replay the first response despite advertising a different
    // operation mode. With path-and-query in the hash, the second call
    // sees a different request_hash and the store reports
    // idempotency_key_conflict (422).
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    cratestack::SqlxIdempotencyStore::new(pool.clone())
        .ensure_schema()
        .await
        .expect("ensure schema");
    let router = build_router(pool.clone());
    let body = r#"{"id":501,"code":"V-501","amount":1}"#;

    let first = router
        .clone()
        .oneshot(
            Request::post("/vouchers?dry_run=true")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .header("idempotency-key", "qs-501")
                .body(Body::from(body))
                .expect("first req"),
        )
        .await
        .expect("first send");
    assert_eq!(first.status(), StatusCode::CREATED);

    let second = router
        .clone()
        .oneshot(
            Request::post("/vouchers?dry_run=false")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .header("idempotency-key", "qs-501")
                .body(Body::from(body))
                .expect("second req"),
        )
        .await
        .expect("second send");

    // The differing query string must produce a fresh request hash,
    // which the existing-row classifier resolves as
    // idempotency_key_conflict (422). Crucially it must NOT be a 201
    // replay — that would prove the query string was discarded.
    assert_eq!(
        second.status(),
        StatusCode::UNPROCESSABLE_ENTITY,
        "different query string under same key must conflict, not replay",
    );
    assert!(
        second.headers().get("idempotency-replayed").is_none(),
        "conflict response must not carry the replay marker",
    );
}

#[tokio::test]
async fn get_requests_bypass_idempotency_layer_entirely() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    cratestack::SqlxIdempotencyStore::new(pool.clone())
        .ensure_schema()
        .await
        .expect("ensure schema");

    // Seed a row to read.
    query("INSERT INTO vouchers VALUES (3, 'V-3', 1)")
        .execute(pool)
        .await
        .expect("seed");
    let router = build_router(pool.clone());

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
