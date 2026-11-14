//! cratestack#1359 on the REST binding (the Dart clients speak CBOR over
//! REST too): a `Uuid` sent as CBOR *text* in a procedure body is accepted,
//! the bytes form still is, and a malformed one is a 400. The RPC twin is
//! `cbor_uuid_text_rpc.rs`.

use cratestack::axum::body::Body;
use cratestack::axum::http::Request;
use cratestack::{CratestackCodec, CratestackContext, CratestackError, include_server_schema};
use cratestack_codec_cbor::CborCodec;
use serde_json::json;
use tower::ServiceExt;

include_server_schema!("tests/fixtures/cbor_uuid_shapes_rest.cstack", db = None);

const TEXT: &str = "01234567-89ab-cdef-0123-456789abcdef";
const UUID: u128 = 0x0123_4567_89ab_cdef_0123_4567_89ab_cdef;

#[derive(Clone, Default)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    async fn echo(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::echo::Args,
        _authorized: cratestack_schema::procedures::echo::Authorized,
    ) -> Result<cratestack_schema::procedures::echo::Output, CratestackError> {
        Ok(args.input)
    }

    async fn echo_id(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::echo_id::Args,
        _authorized: cratestack_schema::procedures::echo_id::Authorized,
    ) -> Result<cratestack_schema::procedures::echo_id::Output, CratestackError> {
        Ok(args.id)
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

async fn post(path: &str, body: Vec<u8>) -> (u16, Vec<u8>) {
    let router = cratestack_schema::axum::router(
        cratestack_schema::Cratestack::builder().build(),
        Procedures,
        (),
        CborCodec,
        AllowAllAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    let request = Request::post(path)
        .header("content-type", CborCodec::CONTENT_TYPE)
        .header("accept", CborCodec::CONTENT_TYPE)
        .body(Body::from(body))
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    let status = response.status().as_u16();
    let bytes = cratestack::axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, bytes.to_vec())
}

fn cbor(value: serde_json::Value) -> Vec<u8> {
    CborCodec.encode(&value).unwrap()
}

#[tokio::test]
async fn text_uuids_are_accepted_in_every_position_and_answered_as_bytes() {
    let item = json!({"id": TEXT, "maybe": TEXT, "ids": [TEXT], "inner": {"ids": [TEXT, TEXT]}});
    let (status, body) = post("/$procs/echo", cbor(json!({"input": item}))).await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let sent: cratestack::Value = CborCodec.decode(&body).unwrap();
    assert!(
        !format!("{sent:?}").contains(TEXT),
        "response must carry bytes: {sent:?}"
    );

    let (status, body) = post("/$procs/echoId", cbor(json!({"id": TEXT}))).await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    assert_eq!(body, [&[0x50u8][..], &UUID.to_be_bytes()[..]].concat());
}

#[tokio::test]
async fn the_bytes_form_is_still_accepted_and_bad_values_are_refused() {
    // {"id": h'<16 bytes>'}
    let mut bytes = vec![0xa1, 0x62, b'i', b'd', 0x50];
    bytes.extend_from_slice(&UUID.to_be_bytes());
    let (status, body) = post("/$procs/echoId", bytes).await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));

    let (status, _) = post("/$procs/echoId", cbor(json!({"id": "nope"}))).await;
    assert_eq!(status, 400);
    let mut short = vec![0xa1, 0x62, b'i', b'd', 0x4f];
    short.extend_from_slice(&[1; 15]);
    let (status, _) = post("/$procs/echoId", short).await;
    assert_eq!(status, 400);
}
