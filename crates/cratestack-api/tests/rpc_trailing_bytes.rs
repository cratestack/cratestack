//! #1153 on the RPC transport: a CBOR body with bytes after its first item is
//! refused before dispatch, on the unary `POST /rpc/{op_id}` route and on
//! `POST /rpc/batch`. Runs on the `db = None` RPC fixture, as
//! `rpc_batch_frame_cap.rs` does, so no database is needed.
//!
//! The RPC binding reports the refusal as `invalid_argument`, the gRPC-style
//! code `cratestack_error_code_to_rpc_code` maps `CODEC_ERROR` to. The HTTP
//! status is the same 400 the REST route returns.

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::rpc::{RPC_BATCH_PATH, RpcRequest};
use cratestack::{CratestackCodec, CratestackContext, CratestackError};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use tower::ServiceExt;

include_server_schema!("tests/fixtures/rpc_batch_no_database.cstack", db = None);

#[derive(Clone, Default)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    fn ping(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::ping::Args,
        _authorized: cratestack_schema::procedures::ping::Authorized,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::ping::Output, CratestackError>,
    > + Send {
        async move {
            Ok(cratestack_schema::PingReply {
                nonce: args.args.nonce,
            })
        }
    }
}

/// Authenticates every caller; the fixture's `ping` requires an authenticated
/// context and nothing here depends on who the caller is.
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

fn cbor_router() -> cratestack::axum::Router {
    let db = cratestack_schema::Cratestack::builder().build();
    cratestack_schema::axum::rpc_router(
        db,
        Procedures,
        (),
        CborCodec,
        AllowAllAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

fn json_router() -> cratestack::axum::Router {
    let db = cratestack_schema::Cratestack::builder().build();
    cratestack_schema::axum::rpc_router(
        db,
        Procedures,
        (),
        JsonCodec,
        AllowAllAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

fn ping_input() -> serde_json::Value {
    serde_json::json!({ "args": { "nonce": "7" } })
}

/// POSTs `body` to `path` as `content_type` and returns the status and the
/// raw response bytes, decoded by the codec that matches `content_type`.
async fn post(
    app: cratestack::axum::Router,
    path: &str,
    content_type: &'static str,
    body: Vec<u8>,
) -> (StatusCode, serde_json::Value) {
    let response = app
        .oneshot(
            Request::post(path)
                .header("content-type", content_type)
                .header("accept", content_type)
                .body(Body::from(body))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed at the transport level");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should buffer");
    let value = if content_type == CborCodec::CONTENT_TYPE {
        CborCodec
            .decode(&bytes)
            .expect("CBOR response should decode")
    } else {
        serde_json::from_slice(&bytes).expect("JSON response should decode")
    };
    (status, value)
}

/// The tails that follow one complete item: a stray byte, a non-item byte
/// run, and a whole second item.
const TAILS: [&[u8]; 3] = [&[0x00], &[0xff, 0xff, 0xde, 0xad], &[0xf6]];

#[tokio::test]
async fn unary_cbor_with_trailing_bytes_is_refused_before_dispatch() {
    let clean = CborCodec.encode(&ping_input()).expect("encode input");

    // Control: the clean item dispatches and echoes its nonce.
    let (status, reply) = post(
        cbor_router(),
        "/rpc/procedure.ping",
        CborCodec::CONTENT_TYPE,
        clean.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply["nonce"], "7");

    for tail in TAILS {
        let mut body = clean.clone();
        body.extend_from_slice(tail);
        let (status, error) = post(
            cbor_router(),
            "/rpc/procedure.ping",
            CborCodec::CONTENT_TYPE,
            body,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "CBOR tail {tail:02x?}");
        assert_eq!(
            error["code"], "invalid_argument",
            "CBOR tail {tail:02x?}: {error}"
        );
    }
}

#[tokio::test]
async fn unary_json_with_trailing_bytes_is_refused_before_dispatch() {
    // JSON parity: the refusal already held for JSON before #1153.
    let mut body = serde_json::to_vec(&ping_input()).expect("encode input");
    let (status, reply) = post(
        json_router(),
        "/rpc/procedure.ping",
        JsonCodec::CONTENT_TYPE,
        body.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply["nonce"], "7");

    body.extend_from_slice(b" x");
    let (status, error) = post(
        json_router(),
        "/rpc/procedure.ping",
        JsonCodec::CONTENT_TYPE,
        body,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["code"], "invalid_argument");
}

#[tokio::test]
async fn batch_cbor_with_trailing_bytes_is_refused_before_dispatch() {
    let frames = vec![RpcRequest {
        id: 1,
        op: "procedure.ping".into(),
        input: ping_input(),
        idem: None,
    }];
    let clean = CborCodec.encode(&frames).expect("encode batch");

    let (status, reply) = post(
        cbor_router(),
        RPC_BATCH_PATH,
        CborCodec::CONTENT_TYPE,
        clean.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply[0]["output"]["nonce"], "7", "{reply}");

    for tail in TAILS {
        let mut body = clean.clone();
        body.extend_from_slice(tail);
        let (status, error) =
            post(cbor_router(), RPC_BATCH_PATH, CborCodec::CONTENT_TYPE, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "batch tail {tail:02x?}");
        assert_eq!(
            error["code"], "invalid_argument",
            "batch tail {tail:02x?}: {error}"
        );
    }
}
