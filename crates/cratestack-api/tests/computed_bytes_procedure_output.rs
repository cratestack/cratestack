//! cratestack#1425: procedure-output composition
//! (`cratestack-macros/src/computed/compose.rs`) builds its leaves by hand,
//! outside the struct's `Serialize`, so a stored `Bytes` field and a
//! `@computed` one each have to be wrapped for the byte-string form there.
//! Over CBOR both are byte strings; over JSON both stay integer arrays.

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::{CratestackCodec, CratestackContext, CratestackError};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use cratestack_core::rpc::RpcFrameValue;
use tower::ServiceExt;

include_server_schema!("tests/fixtures/computed_bytes.cstack", db = None);

#[derive(Clone, Default)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    fn get_blob(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::get_blob::Args,
        _authorized: cratestack_schema::procedures::get_blob::Authorized,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::get_blob::Output, CratestackError>,
    > + Send {
        async move {
            Ok(cratestack_schema::Blob {
                label: args.label,
                data: vec![1, 2, 255],
            })
        }
    }
}

#[derive(Clone)]
struct Resolver;

impl cratestack_schema::ComputedFieldResolver for Resolver {
    fn resolve_blob_digest(
        &self,
        _db: &cratestack_schema::Cratestack,
        _source: &cratestack_schema::Blob,
        _ctx: &CratestackContext,
    ) -> impl core::future::Future<Output = Result<Vec<u8>, CratestackError>> + Send {
        async move { Ok(vec![9, 8, 7]) }
    }
}

#[derive(Clone)]
struct AllowAllAuth;

impl cratestack::AuthProvider for AllowAllAuth {
    type Error = CratestackError;

    fn authenticate(
        &self,
        _request: &cratestack::RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(CratestackContext::authenticated([(
            "id".to_owned(),
            cratestack::Value::Int(1),
        )])))
    }
}

async fn call<C: CratestackCodec + Clone + Send + Sync + 'static>(codec: C) -> Vec<u8> {
    let db = cratestack_schema::Cratestack::builder().build();
    let app = cratestack_schema::axum::router(
        db,
        Procedures,
        Resolver,
        codec.clone(),
        AllowAllAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    let request = codec
        .encode(&serde_json::json!({ "label": "blob" }))
        .expect("encode");
    let response = app
        .oneshot(
            Request::post("/$procs/getBlob")
                .header("content-type", C::CONTENT_TYPE)
                .header("accept", C::CONTENT_TYPE)
                .body(Body::from(request))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec()
}

#[tokio::test]
async fn cbor_writes_stored_and_computed_bytes_as_byte_strings() {
    let body = call(CborCodec).await;
    let RpcFrameValue::Object(fields) = CborCodec.decode::<RpcFrameValue>(&body).unwrap() else {
        panic!("the reply is an object");
    };
    assert_eq!(
        fields["data"],
        RpcFrameValue::Bytes(vec![1, 2, 255]),
        "stored"
    );
    assert_eq!(
        fields["digest"],
        RpcFrameValue::Bytes(vec![9, 8, 7]),
        "computed"
    );
}

#[tokio::test]
async fn json_keeps_both_as_integer_arrays() {
    let body = call(JsonCodec).await;
    let reply: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(reply["data"], serde_json::json!([1, 2, 255]));
    assert_eq!(reply["digest"], serde_json::json!([9, 8, 7]));
}
