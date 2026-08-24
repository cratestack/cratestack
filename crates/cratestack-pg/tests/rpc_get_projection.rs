//! Projection (fields/include/includeFields) over the RPC transport for
//! model `get` operations — proves that RPC `model.<X>.get` now carries
//! the same projection semantics as REST `GET /<plural>/{id}`, which was
//! previously asymmetric (docs/design/rpc-transport.md §3.1a).
//!
//! This fixture includes both a to-one and to-many relation pair
//! (`RpcProjPhoto` → `RpcProjAlbum`, and the reverse) so the full
//! include/include_fields composition can be tested end-to-end, including
//! field narrowing on included relations.
//!
//! PG-gated: skips silently without `CRATESTACK_TEST_DATABASE_URL` /
//! `CRATESTACK_USE_TESTCONTAINERS`, same pattern as every other PG
//! integration test in this crate (see `tests/support/pg.rs`).

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::rpc::{RpcGetInput, RpcRequest, RpcResponseFrame};
use cratestack::sqlx::query;
use cratestack::{
    AuthProvider, CratestackCodec, CratestackContext, CratestackError, RequestContext, Value,
};
use cratestack_codec_json::JsonCodec;
use tower::util::ServiceExt;

include_server_schema!("tests/fixtures/rpc_get_projection.cstack", db = Postgres);

mod support;

use support::pg;

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS rpc_proj_photos")
        .execute(pool)
        .await
        .expect("drop photos table");
    query("DROP TABLE IF EXISTS rpc_proj_albums")
        .execute(pool)
        .await
        .expect("drop albums table");
    query(
        "CREATE TABLE rpc_proj_albums (
            id BIGINT PRIMARY KEY,
            title TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await
    .expect("create rpc_proj_albums");
    query(
        "CREATE TABLE rpc_proj_photos (
            id BIGINT PRIMARY KEY,
            album_id BIGINT NOT NULL,
            storage_key TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await
    .expect("create rpc_proj_photos");
}

async fn seed(pool: &cratestack::sqlx::PgPool) {
    query(
        "INSERT INTO rpc_proj_albums (id, title) VALUES \
         (1, 'Holiday')",
    )
    .execute(pool)
    .await
    .expect("seed albums");
    query(
        "INSERT INTO rpc_proj_photos (id, album_id, storage_key) VALUES \
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

#[derive(Clone)]
struct NoProcedures;

impl cratestack_schema::procedures::ProcedureRegistry for NoProcedures {}

/// Resolves `proxyUrl` to a deterministic URL derived from `storageKey`
/// (and, when supplied, `width`), and `thumbnailUrl` to a fixed derivative.
#[derive(Clone)]
struct TestResolver;

impl cratestack_schema::ComputedFieldResolver for TestResolver {
    fn resolve_rpc_proj_photo_proxy_url(
        &self,
        _db: &cratestack_schema::Cratestack,
        source: &cratestack_schema::RpcProjPhoto,
        params: Option<&cratestack_schema::RpcProjProxyParams>,
        _ctx: &CratestackContext,
    ) -> impl core::future::Future<Output = Result<String, CratestackError>> + Send {
        let storage_key = source.storageKey.clone();
        let width = params.and_then(|p| p.width);
        async move {
            Ok(match width {
                Some(width) => format!("https://cdn.example/{storage_key}?w={width}"),
                None => format!("https://cdn.example/{storage_key}"),
            })
        }
    }

    fn resolve_rpc_proj_photo_thumbnail_url(
        &self,
        _db: &cratestack_schema::Cratestack,
        source: &cratestack_schema::RpcProjPhoto,
        _ctx: &CratestackContext,
    ) -> impl core::future::Future<Output = Result<String, CratestackError>> + Send {
        let storage_key = source.storageKey.clone();
        async move { Ok(format!("https://cdn.example/thumb/{storage_key}")) }
    }
}

fn test_router(pool: &cratestack::sqlx::PgPool) -> cratestack::axum::Router {
    let db = cratestack_schema::Cratestack::builder(pool.clone()).build();
    cratestack_schema::axum::rpc_router(
        db,
        NoProcedures,
        TestResolver,
        JsonCodec,
        PassThroughAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

async fn rpc_unary(
    router: cratestack::axum::Router,
    op_id: &str,
    body: Vec<u8>,
) -> (StatusCode, cratestack::serde_json::Value) {
    let response = router
        .oneshot(
            Request::post(format!("/rpc/{op_id}"))
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::from(body))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should read");
    let value: cratestack::serde_json::Value =
        cratestack::serde_json::from_slice(&bytes).expect("response should decode as JSON");
    (status, value)
}

// ----- Test 1: fields projection -----

#[tokio::test]
async fn rpc_get_projects_fields() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let router = test_router(pool);
    let input = RpcGetInput {
        id: 1i64,
        fields: Some(vec!["id".into(), "storageKey".into()]),
        ..Default::default()
    };
    let body = JsonCodec.encode(&input).expect("get input should encode");

    let (status, value) = rpc_unary(router, "model.RpcProjPhoto.get", body).await;
    assert_eq!(status, StatusCode::OK);
    assert!(value.get("id").is_some(), "id should be present");
    assert!(
        value.get("storageKey").is_some(),
        "storageKey should be present"
    );
    assert!(
        value.get("thumbnailUrl").is_none(),
        "thumbnailUrl should be absent (not in fields)"
    );
    assert_eq!(
        value.as_object().unwrap().len(),
        2,
        "object should have exactly 2 keys"
    );
}

// ----- Test 2: include a relation -----

#[tokio::test]
async fn rpc_get_includes_a_relation() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let router = test_router(pool);
    let input = RpcGetInput {
        id: 1i64,
        include: Some(vec!["album".into()]),
        ..Default::default()
    };
    let body = JsonCodec.encode(&input).expect("get input should encode");

    let (status, value) = rpc_unary(router, "model.RpcProjPhoto.get", body).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        value
            .get("album")
            .and_then(|a| a.get("title"))
            .and_then(|t| t.as_str()),
        Some("Holiday"),
        "album.title should be 'Holiday'"
    );
}

// ----- Test 3: include_fields narrows the included relation -----

#[tokio::test]
async fn rpc_get_include_fields_narrows_the_included_relation() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let router = test_router(pool);
    let mut include_fields = std::collections::BTreeMap::new();
    include_fields.insert("album".to_owned(), vec!["id".to_owned()]);
    let input = RpcGetInput {
        id: 1i64,
        include: Some(vec!["album".into()]),
        include_fields,
        ..Default::default()
    };
    let body = JsonCodec.encode(&input).expect("get input should encode");

    let (status, value) = rpc_unary(router, "model.RpcProjPhoto.get", body).await;
    assert_eq!(status, StatusCode::OK);
    let album = value.get("album").expect("album should be present");
    assert!(
        album.get("id").is_some(),
        "album.id should be present (in includeFields)"
    );
    assert!(
        album.get("title").is_none(),
        "album.title should be absent (not in includeFields)"
    );
}

// ----- Test 4: HEADLINE TEST — reject computedParams for excluded fields -----

#[tokio::test]
async fn rpc_get_rejects_computed_params_for_a_field_excluded_by_fields() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let router = test_router(pool);
    let input = RpcGetInput {
        id: 1i64,
        fields: Some(vec!["id".into()]),
        computed_params: Some(r#"{"proxyUrl":{"width":800}}"#.to_owned()),
        ..Default::default()
    };
    let body = JsonCodec.encode(&input).expect("get input should encode");

    let (status, _value) = rpc_unary(router, "model.RpcProjPhoto.get", body).await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "computedParams for a field excluded by fields should be rejected with 422. \
         This branch was UNREACHABLE over RPC before T1 — the headline test."
    );
}

// ----- Test 5: reject unknown field name -----

#[tokio::test]
async fn rpc_get_rejects_an_unknown_field_name() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let router = test_router(pool);
    let input = RpcGetInput {
        id: 1i64,
        fields: Some(vec!["nope".into()]),
        ..Default::default()
    };
    let body = JsonCodec.encode(&input).expect("get input should encode");

    let (status, _value) = rpc_unary(router, "model.RpcProjPhoto.get", body).await;
    assert!(!status.is_success(), "unknown field should be rejected");
}

// ----- Test 6: fields and computed_params compose -----

#[tokio::test]
async fn rpc_get_fields_and_computed_params_compose() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let router = test_router(pool);
    let input = RpcGetInput {
        id: 1i64,
        fields: Some(vec!["id".into(), "proxyUrl".into()]),
        computed_params: Some(r#"{"proxyUrl":{"width":800}}"#.to_owned()),
        ..Default::default()
    };
    let body = JsonCodec.encode(&input).expect("get input should encode");

    let (status, value) = rpc_unary(router, "model.RpcProjPhoto.get", body).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        value
            .get("proxyUrl")
            .and_then(|p| p.as_str())
            .map(|s| s.ends_with("?w=800"))
            .unwrap_or(false),
        "proxyUrl should end with ?w=800"
    );
    assert!(
        value.get("storageKey").is_none(),
        "storageKey should be absent (not in fields)"
    );
}

// ----- Test 7: without selection returns full record -----

#[tokio::test]
async fn rpc_get_without_selection_returns_the_full_record() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let router = test_router(pool);
    let input = RpcGetInput {
        id: 1i64,
        ..Default::default()
    };
    let body = JsonCodec.encode(&input).expect("get input should encode");

    let (status, value) = rpc_unary(router, "model.RpcProjPhoto.get", body).await;
    assert_eq!(status, StatusCode::OK);
    assert!(value.get("id").is_some(), "id should be present");
    assert!(value.get("albumId").is_some(), "albumId should be present");
    assert!(
        value.get("storageKey").is_some(),
        "storageKey should be present"
    );
    assert!(
        value.get("proxyUrl").is_some(),
        "proxyUrl should be present"
    );
    assert!(
        value.get("thumbnailUrl").is_some(),
        "thumbnailUrl should be present"
    );
}

// ----- Test 8: batch frames with independent projections -----

#[tokio::test]
async fn rpc_batch_frames_carry_independent_projections() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let router = test_router(pool);
    let frames = vec![
        RpcRequest {
            id: 1,
            op: "model.RpcProjPhoto.get".to_owned(),
            input: cratestack::serde_json::json!({
                "id": 1,
                "fields": ["id"],
            }),
            idem: None,
        },
        RpcRequest {
            id: 2,
            op: "model.RpcProjPhoto.get".to_owned(),
            input: cratestack::serde_json::json!({
                "id": 2,
                "fields": ["id", "storageKey"],
            }),
            idem: None,
        },
    ];
    let body = JsonCodec.encode(&frames).expect("batch body should encode");

    let response = router
        .oneshot(
            Request::post("/rpc/batch")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::from(body))
                .expect("request should build"),
        )
        .await
        .expect("batch request should succeed");
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should read");
    let responses: Vec<RpcResponseFrame> = JsonCodec
        .decode(&bytes)
        .expect("batch response should decode");
    assert_eq!(responses.len(), 2);

    assert_eq!(responses[0].id, 1);
    assert!(responses[0].error.is_none(), "frame 0: {:?}", responses[0]);
    let out0 = responses[0].output.as_ref().expect("frame 0 has output");
    assert_eq!(
        out0.as_object().unwrap().len(),
        1,
        "frame 0 output should have exactly 1 key (id only)"
    );

    assert_eq!(responses[1].id, 2);
    assert!(responses[1].error.is_none(), "frame 1: {:?}", responses[1]);
    let out1 = responses[1].output.as_ref().expect("frame 1 has output");
    assert_eq!(
        out1.as_object().unwrap().len(),
        2,
        "frame 1 output should have exactly 2 keys (id and storageKey)"
    );
}
