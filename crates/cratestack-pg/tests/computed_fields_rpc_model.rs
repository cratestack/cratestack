//! `computedParams` and full selection surface (`fields`, `include`,
//! `includeFields`) over the RPC transport for model reads
//! (`model.<X>.get`/`model.<X>.list`), and per-frame over `/rpc/batch` —
//! Stage 1 of the plan in `docs/design/rpc-transport.md`'s `RpcGetInput`/
//! `RpcListInput` sections.
//!
//! `computed_fields_rpc.cstack`/`computed_fields_rpc.rs` already proves RPC
//! composes computed fields on *procedure* outputs; this fixture is the
//! missing model-CRUD-over-RPC counterpart, mirroring
//! `computed_fields_router.cstack`/`computed_fields_router.rs`'s REST
//! coverage of the same feature but dispatched through `rpc_router`
//! instead of `model_router`.
//!
//! PG-gated: skips silently without `CRATESTACK_TEST_DATABASE_URL` /
//! `CRATESTACK_USE_TESTCONTAINERS`, same pattern as every other PG
//! integration test in this crate (see `tests/support/pg.rs`).

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::rpc::{RpcGetInput, RpcListInput, RpcRequest, RpcResponseFrame};
use cratestack::sqlx::query;
use cratestack::{
    AuthProvider, CratestackCodec, CratestackContext, CratestackError, RequestContext, Value,
};
use cratestack_codec_json::JsonCodec;
use tower::util::ServiceExt;

include_server_schema!(
    "tests/fixtures/computed_fields_rpc_model.cstack",
    db = Postgres
);

mod support;

use support::pg;

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS comp_rpc_photos")
        .execute(pool)
        .await
        .expect("drop table");
    query(
        "CREATE TABLE comp_rpc_photos (
            id BIGINT PRIMARY KEY,
            storage_key TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await
    .expect("create comp_rpc_photos");
}

async fn seed(pool: &cratestack::sqlx::PgPool) {
    query(
        "INSERT INTO comp_rpc_photos (id, storage_key) VALUES \
         (1, 'media/one.png'), (2, 'media/two.png')",
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
/// (and, when supplied, `width`) — same shape as `computed_fields_router.rs`'s
/// `CountingResolver`, minus the invocation counter (not needed here).
#[derive(Clone)]
struct TestResolver;

impl cratestack_schema::ComputedFieldResolver for TestResolver {
    fn resolve_comp_rpc_photo_proxy_url(
        &self,
        _db: &cratestack_schema::Cratestack,
        source: &cratestack_schema::CompRpcPhoto,
        params: Option<&cratestack_schema::CompRpcProxyParams>,
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

    fn resolve_comp_rpc_photo_thumbnail_url(
        &self,
        _db: &cratestack_schema::Cratestack,
        source: &cratestack_schema::CompRpcPhoto,
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

// ----- Case 1: get, with and without computedParams -----

#[tokio::test]
async fn rpc_get_with_computed_params_reflects_width() {
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
        computed_params: Some(r#"{"proxyUrl":{"width":800}}"#.to_owned()),
        ..Default::default()
    };
    let body = JsonCodec.encode(&input).expect("get input should encode");

    let (status, value) = rpc_unary(router, "model.CompRpcPhoto.get", body).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        value.get("proxyUrl"),
        Some(&cratestack::serde_json::Value::from(
            "https://cdn.example/media/one.png?w=800"
        ))
    );
}

#[tokio::test]
async fn rpc_get_without_computed_params_uses_default() {
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
        computed_params: None,
        ..Default::default()
    };
    let body = JsonCodec.encode(&input).expect("get input should encode");

    let (status, value) = rpc_unary(router, "model.CompRpcPhoto.get", body).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        value.get("proxyUrl"),
        Some(&cratestack::serde_json::Value::from(
            "https://cdn.example/media/one.png"
        )),
        "no computedParams => resolver observes params: None => default value"
    );
}

// ----- Case 2: list, computedParams applies per row -----

#[tokio::test]
async fn rpc_list_with_computed_params_reflects_width_per_row() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let router = test_router(pool);
    let input = RpcListInput {
        sort: Some("id".to_owned()),
        computed_params: Some(r#"{"proxyUrl":{"width":600}}"#.to_owned()),
        ..Default::default()
    };
    let body = JsonCodec.encode(&input).expect("list input should encode");

    let (status, value) = rpc_unary(router, "model.CompRpcPhoto.list", body).await;
    assert_eq!(status, StatusCode::OK);
    let items = value.as_array().expect("list response should be an array");
    assert_eq!(items.len(), 2);
    assert_eq!(
        items[0].get("proxyUrl"),
        Some(&cratestack::serde_json::Value::from(
            "https://cdn.example/media/one.png?w=600"
        ))
    );
    assert_eq!(
        items[1].get("proxyUrl"),
        Some(&cratestack::serde_json::Value::from(
            "https://cdn.example/media/two.png?w=600"
        ))
    );
}

// ----- Case 3: rejection parity with REST -----

#[tokio::test]
async fn rpc_get_rejects_computed_params_naming_a_bare_field() {
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
        computed_params: Some(r#"{"thumbnailUrl":{}}"#.to_owned()),
        ..Default::default()
    };
    let body = JsonCodec.encode(&input).expect("get input should encode");

    let (status, _value) = rpc_unary(router, "model.CompRpcPhoto.get", body).await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "same validation-error status REST returns for a computedParams key \
         naming a param-less computed field"
    );
}

#[tokio::test]
async fn rpc_get_rejects_malformed_computed_params_json() {
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
        computed_params: Some("not-json".to_owned()),
        ..Default::default()
    };
    let body = JsonCodec.encode(&input).expect("get input should encode");

    let (status, _value) = rpc_unary(router, "model.CompRpcPhoto.get", body).await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "same validation-error status REST returns for malformed computedParams JSON"
    );
}

// ----- Case 4: batch, two frames with different computedParams -----

#[tokio::test]
async fn rpc_batch_frames_carry_independent_computed_params() {
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
            op: "model.CompRpcPhoto.get".to_owned(),
            input: cratestack::serde_json::json!({
                "id": 1,
                "computedParams": r#"{"proxyUrl":{"width":100}}"#,
            }),
            idem: None,
        },
        RpcRequest {
            id: 2,
            op: "model.CompRpcPhoto.get".to_owned(),
            input: cratestack::serde_json::json!({
                "id": 2,
                "computedParams": r#"{"proxyUrl":{"width":200}}"#,
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
        out0.get("proxyUrl"),
        Some(&cratestack::serde_json::Value::from(
            "https://cdn.example/media/one.png?w=100"
        ))
    );

    assert_eq!(responses[1].id, 2);
    assert!(responses[1].error.is_none(), "frame 1: {:?}", responses[1]);
    let out1 = responses[1].output.as_ref().expect("frame 1 has output");
    assert_eq!(
        out1.get("proxyUrl"),
        Some(&cratestack::serde_json::Value::from(
            "https://cdn.example/media/two.png?w=200"
        )),
        "each batch frame's computedParams must resolve independently, in order"
    );
}

// ----- Case 5: backward compat — bare `{"id": 1}` frame still dispatches -----

#[tokio::test]
async fn rpc_get_bare_id_frame_still_dispatches() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let router = test_router(pool);
    // Deliberately NOT `RpcGetInput` — this is the exact `{"id": 1}` shape
    // an old client (pre-computedParams) would have sent.
    let body = JsonCodec
        .encode(&cratestack::serde_json::json!({ "id": 1 }))
        .expect("bare id input should encode");

    let (status, value) = rpc_unary(router, "model.CompRpcPhoto.get", body).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        value.get("proxyUrl"),
        Some(&cratestack::serde_json::Value::from(
            "https://cdn.example/media/one.png"
        )),
        "an old bare {{id}} frame must still dispatch and resolve with params: None"
    );
}
