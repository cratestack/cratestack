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
    PgPoolOptions::new()
        .max_connections(2)
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
