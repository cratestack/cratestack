//! End-to-end HTTP coverage for `@computed` model-field resolution
//! through the generated axum router (docs/design/computed-fields.md).
//! PG-gated: skips silently without `CRATESTACK_TEST_DATABASE_URL` /
//! `CRATESTACK_USE_TESTCONTAINERS`, same pattern as every other PG
//! integration test in this crate (see `tests/support/pg.rs`).
//!
//! Covers, against a real router:
//! - GET returns the resolved computed field.
//! - `?fields=` excluding the computed field omits it on the wire AND
//!   skips resolver invocation entirely (observed via a counting
//!   resolver).
//! - `?computedParams=` changes the resolved value.
//! - Malformed/illegal `?computedParams=` values return 422
//!   (`CratestackError::Validation`'s mapped status).
//! - `POST` (create) response includes the resolved computed field.
//! - `GET` list resolves the computed field per row.
//! - A to-many relation include resolves the related model's own
//!   computed field with `params: None` (v1 never threads root
//!   `computedParams` into includes).

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::sqlx::query;
use cratestack::{
    AuthProvider, CratestackCodec, CratestackContext, CratestackError, RequestContext, Value,
};
use cratestack_codec_json::JsonCodec;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::util::ServiceExt;

include_server_schema!(
    "tests/fixtures/computed_fields_router.cstack",
    db = Postgres
);

mod support;

use support::pg;

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS comp_router_photos, comp_router_albums")
        .execute(pool)
        .await
        .expect("drop tables");
    query("CREATE TABLE comp_router_albums (id BIGINT PRIMARY KEY, title TEXT NOT NULL)")
        .execute(pool)
        .await
        .expect("create comp_router_albums");
    query(
        "CREATE TABLE comp_router_photos (
            id BIGINT PRIMARY KEY,
            album_id BIGINT NOT NULL,
            storage_key TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await
    .expect("create comp_router_photos");
}

async fn seed(pool: &cratestack::sqlx::PgPool) {
    query("INSERT INTO comp_router_albums (id, title) VALUES (1, 'Vacation')")
        .execute(pool)
        .await
        .expect("seed album");
    query(
        "INSERT INTO comp_router_photos (id, album_id, storage_key) VALUES \
         (1, 1, 'media/one.png'), (2, 1, 'media/two.png')",
    )
    .execute(pool)
    .await
    .expect("seed photos");
}

#[derive(Clone)]
struct PassThroughAuth;

impl AuthProvider for PassThroughAuth {
    type Error = CratestackError;
    fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(CratestackContext::authenticated([(
            "id".to_owned(),
            Value::Int(1),
        )])))
    }
}

/// Resolves `proxyUrl` to a deterministic URL derived from `storageKey`
/// (and, when supplied, `width`), and counts every invocation — so tests
/// can assert a `?fields=`-excluded computed field is never resolved at
/// all, not merely omitted from the response after being computed.
#[derive(Clone)]
struct CountingResolver {
    invocations: Arc<AtomicUsize>,
}

impl CountingResolver {
    fn new() -> Self {
        Self {
            invocations: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl cratestack_schema::ComputedFieldResolver for CountingResolver {
    fn resolve_comp_router_photo_proxy_url(
        &self,
        _db: &cratestack_schema::Cratestack,
        source: &cratestack_schema::CompRouterPhoto,
        params: Option<&cratestack_schema::CompRouterProxyParams>,
        _ctx: &CratestackContext,
    ) -> impl core::future::Future<Output = Result<String, cratestack::CratestackError>> + Send
    {
        self.invocations.fetch_add(1, Ordering::SeqCst);
        let storage_key = source.storageKey.clone();
        let width = params.and_then(|p| p.width);
        async move {
            Ok(match width {
                Some(width) => format!("https://cdn.example/{storage_key}?w={width}"),
                None => format!("https://cdn.example/{storage_key}"),
            })
        }
    }

    fn resolve_comp_router_photo_thumbnail_url(
        &self,
        _db: &cratestack_schema::Cratestack,
        source: &cratestack_schema::CompRouterPhoto,
        _ctx: &CratestackContext,
    ) -> impl core::future::Future<Output = Result<String, cratestack::CratestackError>> + Send
    {
        let storage_key = source.storageKey.clone();
        async move { Ok(format!("https://cdn.example/thumb/{storage_key}")) }
    }
}

fn test_db(pool: &cratestack::sqlx::PgPool) -> cratestack_schema::Cratestack {
    cratestack_schema::Cratestack::builder(pool.clone()).build()
}

#[tokio::test]
async fn get_returns_the_resolved_computed_field() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let resolver = CountingResolver::new();
    let router =
        cratestack_schema::axum::model_router(test_db(pool), resolver, JsonCodec, PassThroughAuth);

    let response = router
        .oneshot(
            Request::get("/comp_router_photos/1")
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should read");
    let value: cratestack::serde_json::Value =
        cratestack::serde_json::from_slice(&body).expect("response should decode as JSON");
    assert_eq!(
        value.get("proxyUrl"),
        Some(&cratestack::serde_json::Value::from(
            "https://cdn.example/media/one.png"
        ))
    );
}

#[tokio::test]
async fn fields_selection_excluding_computed_field_skips_resolution() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let resolver = CountingResolver::new();
    let invocations = resolver.invocations.clone();
    let router =
        cratestack_schema::axum::model_router(test_db(pool), resolver, JsonCodec, PassThroughAuth);

    let response = router
        .oneshot(
            Request::get("/comp_router_photos/1?fields=id,storageKey")
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should read");
    let value: cratestack::serde_json::Value =
        cratestack::serde_json::from_slice(&body).expect("response should decode as JSON");
    assert_eq!(
        value.get("proxyUrl"),
        None,
        "excluded computed field must not appear on the wire"
    );
    assert_eq!(
        invocations.load(Ordering::SeqCst),
        0,
        "excluded computed field's resolver must never be invoked"
    );
}

#[tokio::test]
async fn computed_params_changes_the_resolved_value() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let resolver = CountingResolver::new();
    let router =
        cratestack_schema::axum::model_router(test_db(pool), resolver, JsonCodec, PassThroughAuth);

    let response = router
        .oneshot(
            Request::get("/comp_router_photos/1?computedParams=%7B%22proxyUrl%22%3A%7B%22width%22%3A800%7D%7D")
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should read");
    let value: cratestack::serde_json::Value =
        cratestack::serde_json::from_slice(&body).expect("response should decode as JSON");
    assert_eq!(
        value.get("proxyUrl"),
        Some(&cratestack::serde_json::Value::from(
            "https://cdn.example/media/one.png?w=800"
        ))
    );
}

#[tokio::test]
async fn invalid_computed_params_returns_unprocessable_entity() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let resolver = CountingResolver::new();
    let router =
        cratestack_schema::axum::model_router(test_db(pool), resolver, JsonCodec, PassThroughAuth);

    // Malformed JSON.
    let response = router
        .clone()
        .oneshot(
            Request::get("/comp_router_photos/1?computedParams=not-json")
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    // Unknown field key.
    let response = router
        .clone()
        .oneshot(
            Request::get("/comp_router_photos/1?computedParams=%7B%22nope%22%3A%7B%7D%7D")
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn create_response_includes_the_resolved_computed_field() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let resolver = CountingResolver::new();
    let router =
        cratestack_schema::axum::model_router(test_db(pool), resolver, JsonCodec, PassThroughAuth);

    let create_body = cratestack::serde_json::json!({
        "id": 3,
        "albumId": 1,
        "storageKey": "media/three.png",
    });
    let response = router
        .oneshot(
            Request::post("/comp_router_photos")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::from(create_body.to_string()))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(response.status(), StatusCode::CREATED);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should read");
    let value: cratestack::serde_json::Value =
        cratestack::serde_json::from_slice(&body).expect("response should decode as JSON");
    assert_eq!(
        value.get("proxyUrl"),
        Some(&cratestack::serde_json::Value::from(
            "https://cdn.example/media/three.png"
        ))
    );
}

#[tokio::test]
async fn list_resolves_the_computed_field_per_row() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let resolver = CountingResolver::new();
    let router =
        cratestack_schema::axum::model_router(test_db(pool), resolver, JsonCodec, PassThroughAuth);

    let response = router
        .oneshot(
            Request::get("/comp_router_photos?sort=id")
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should read");
    let values: Vec<cratestack::serde_json::Value> =
        cratestack::serde_json::from_slice(&body).expect("response should decode as JSON array");
    assert_eq!(values.len(), 2);
    assert_eq!(
        values[0].get("proxyUrl"),
        Some(&cratestack::serde_json::Value::from(
            "https://cdn.example/media/one.png"
        ))
    );
    assert_eq!(
        values[1].get("proxyUrl"),
        Some(&cratestack::serde_json::Value::from(
            "https://cdn.example/media/two.png"
        ))
    );
}

#[tokio::test]
async fn relation_include_resolves_related_models_computed_field_with_none_params() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let resolver = CountingResolver::new();
    let router =
        cratestack_schema::axum::model_router(test_db(pool), resolver, JsonCodec, PassThroughAuth);

    let response = router
        .oneshot(
            Request::get("/comp_router_albums/1?include=photos")
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should read");
    let value: cratestack::serde_json::Value =
        cratestack::serde_json::from_slice(&body).expect("response should decode as JSON");
    let photos = value
        .get("photos")
        .and_then(|value| value.as_array())
        .expect("photos include should be a present array");
    assert_eq!(photos.len(), 2);
    // Included records always resolve with `params: None` in v1
    // (docs/design/computed-fields.md) — no `computedParams` was
    // supplied on this request at all, so this also proves includes
    // don't silently inherit the root's (absent) params.
    let urls: Vec<&str> = photos
        .iter()
        .map(|photo| {
            photo
                .get("proxyUrl")
                .and_then(|value| value.as_str())
                .expect("each included photo should carry a resolved proxyUrl")
        })
        .collect();
    assert_eq!(
        urls,
        vec![
            "https://cdn.example/media/one.png",
            "https://cdn.example/media/two.png",
        ]
    );
}
