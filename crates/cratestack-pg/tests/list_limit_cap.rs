//! Regression test for #123: a generated list route's `limit` query
//! parameter had no upper bound — a caller could request an
//! arbitrarily large `limit` and force an unbounded fetch (and, for
//! `@@paged` models, an unbounded COUNT alongside it). This exercises
//! the real generated Axum route (not the direct Rust query builder,
//! which was never in scope for the cap — see `MAX_LIST_LIMIT`'s doc
//! comment) and confirms both sides of the new boundary.

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::{AuthProvider, CoolCodec, CoolContext, RequestContext, Value};
use cratestack_codec_json::JsonCodec;
use tower::util::ServiceExt;

include_server_schema!("tests/fixtures/list_limit_cap.cstack", db = Postgres);

mod support;

use support::pg;

#[derive(Clone)]
struct AlwaysAuthProvider;

impl AuthProvider for AlwaysAuthProvider {
    type Error = cratestack::CoolError;

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

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    cratestack::sqlx::query("DROP TABLE IF EXISTS items")
        .execute(pool)
        .await
        .expect("drop items table");
    cratestack::sqlx::query("CREATE TABLE items (id BIGINT PRIMARY KEY, label TEXT NOT NULL)")
        .execute(pool)
        .await
        .expect("create items table");
}

#[tokio::test]
async fn list_route_rejects_limit_above_max_list_limit() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let router = cratestack_schema::axum::model_router(
        cratestack_schema::Cratestack::builder(pool.clone()).build(),
        JsonCodec,
        AlwaysAuthProvider,
    );

    let over_limit = cratestack::MAX_LIST_LIMIT + 1;
    let response = router
        .clone()
        .oneshot(
            Request::get(format!("/items?limit={over_limit}"))
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("route request should complete");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should read");
    let error: cratestack::CoolErrorResponse = JsonCodec
        .decode(&body)
        .expect("error envelope should decode");
    assert_eq!(error.code, "BAD_REQUEST");
    assert!(
        error
            .message
            .contains(&cratestack::MAX_LIST_LIMIT.to_string()),
        "error message should name the actual limit: {}",
        error.message,
    );
}

#[tokio::test]
async fn list_route_defaults_omitted_limit_to_max_list_limit() {
    // The oversized-limit rejection above is trivially bypassed by
    // just not sending `limit` at all — that has to default to the
    // same ceiling, not fall through to "no limit."
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    cratestack::sqlx::query("INSERT INTO items (id, label) VALUES (1, 'only-item')")
        .execute(pool)
        .await
        .expect("seed");

    let router = cratestack_schema::axum::model_router(
        cratestack_schema::Cratestack::builder(pool.clone()).build(),
        JsonCodec,
        AlwaysAuthProvider,
    );

    let response = router
        .clone()
        .oneshot(
            Request::get("/items")
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("route request should complete");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should read");
    let page: cratestack::Page<cratestack_schema::Item> =
        JsonCodec.decode(&body).expect("page should decode");
    assert_eq!(
        page.page_info.limit,
        Some(cratestack::MAX_LIST_LIMIT),
        "an omitted limit must default to MAX_LIST_LIMIT, not stay None/unbounded",
    );
}

#[tokio::test]
async fn list_route_allows_limit_exactly_at_max_list_limit() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    cratestack::sqlx::query("INSERT INTO items (id, label) VALUES (1, 'only-item')")
        .execute(pool)
        .await
        .expect("seed");

    let router = cratestack_schema::axum::model_router(
        cratestack_schema::Cratestack::builder(pool.clone()).build(),
        JsonCodec,
        AlwaysAuthProvider,
    );

    let response = router
        .clone()
        .oneshot(
            Request::get(format!("/items?limit={}", cratestack::MAX_LIST_LIMIT))
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("route request should complete");

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "a limit exactly at the cap must succeed, not be rejected off-by-one",
    );
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should read");
    let page: cratestack::Page<cratestack_schema::Item> =
        JsonCodec.decode(&body).expect("page should decode");
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.page_info.limit, Some(cratestack::MAX_LIST_LIMIT));
}
