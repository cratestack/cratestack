//! Proves the `transport rpc` unary dispatch path
//! (`POST /rpc/procedure.<name>`) also composes `@computed` fields into
//! a procedure's response — `docs/design/computed-fields.md`'s
//! "Procedure outputs" section. `crates/cratestack-macros/src/transport/
//! rpc.rs::generate_procedure_rpc_dispatch_arm` constructs a fresh
//! `ProcedureRouterState` from the shared `RpcRouterState` and calls the
//! exact same `handle_<procedure>_dispatch` fn the REST mount uses (see
//! that file's own doc comment) — this test is the empirical proof that
//! reuse actually holds for the composition path added here, not just a
//! read of the generator source.

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::{AuthProvider, CratestackContext, CratestackError, RequestContext, Value};
use cratestack_codec_cbor::CborCodec;
use tower::util::ServiceExt;

include_server_schema!("tests/fixtures/computed_fields_rpc.cstack", db = Postgres);

fn test_db() -> cratestack_schema::Cratestack {
    let pool = cratestack::sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    cratestack_schema::Cratestack::builder(pool).build()
}

#[derive(Clone)]
struct AllowAllAuth;

impl AuthProvider for AllowAllAuth {
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
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    async fn get_image(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::get_image::Args,
        _authorized: cratestack_schema::procedures::get_image::Authorized,
    ) -> Result<cratestack_schema::procedures::get_image::Output, CratestackError> {
        Ok(cratestack_schema::Image {
            storageKey: args.storageKey,
        })
    }
}

#[derive(Clone)]
struct TestComputedFieldResolver;

impl cratestack_schema::ComputedFieldResolver for TestComputedFieldResolver {
    fn resolve_image_thumbnail_url(
        &self,
        _db: &cratestack_schema::Cratestack,
        source: &cratestack_schema::Image,
        _ctx: &CratestackContext,
    ) -> impl core::future::Future<Output = Result<String, CratestackError>> + Send {
        let storage_key = source.storageKey.clone();
        async move { Ok(format!("https://imgproxy.example/{storage_key}")) }
    }
}

#[tokio::test]
async fn rpc_unary_dispatch_composes_computed_fields() {
    use cratestack::CratestackCodec;

    let codec = CborCodec;
    let router = cratestack_schema::axum::rpc_router(
        test_db(),
        Procedures,
        TestComputedFieldResolver,
        codec.clone(),
        AllowAllAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );

    let frame = codec
        .encode(&cratestack_schema::procedures::get_image::Args {
            storageKey: "media/original.png".to_owned(),
        })
        .expect("request frame should encode");

    let response = router
        .oneshot(
            Request::post("/rpc/procedure.getImage")
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("content-type", CborCodec::CONTENT_TYPE)
                .body(Body::from(frame))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should read");
    let value: cratestack::serde_json::Value =
        codec.decode(&bytes).expect("response should decode");
    assert_eq!(
        value.get("storageKey"),
        Some(&cratestack::serde_json::Value::from("media/original.png"))
    );
    assert_eq!(
        value.get("thumbnailUrl"),
        Some(&cratestack::serde_json::Value::from(
            "https://imgproxy.example/media/original.png"
        )),
        "RPC unary dispatch must compose the computed field, same as REST"
    );
}
