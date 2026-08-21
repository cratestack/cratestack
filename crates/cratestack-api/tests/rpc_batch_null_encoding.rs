//! cratestack#657 — decisive regression test: `POST /rpc/batch` must not
//! mis-encode `null` as the CBOR empty-array marker (`0x80`) in either
//! direction. See `crates/cratestack-codec-cbor/src/lib.rs` for the root
//! cause (`serde_json::Value::Null` calls `serialize_unit()`, which
//! `minicbor-serde` encodes as `0x80` by default, not RFC 8949 null
//! `0xf6`) and `crates/cratestack-macros/src/include/server/rpc_module/batch.rs`
//! / `crates/cratestack-axum/src/rpc/batch.rs` for where the batch path's
//! opaque `serde_json::Value` frames used to carry that bug onto the wire.
//!
//! Deliberately asserts raw WIRE BYTES, not decoded values: decoding a
//! corrupted `0x80` back into `Option<String>` either errors outright (for
//! a scalar-typed field) or silently produces the wrong value depending on
//! the target type — decoding is exactly what hides this bug, so this test
//! never gets to decode as its primary evidence.

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::rpc::{RPC_BATCH_PATH, RpcRequest, RpcResponseFrame};
use cratestack::{CratestackCodec, CratestackContext, CratestackError, include_server_schema};
use cratestack_codec_cbor::CborCodec;
use tower::ServiceExt;

include_server_schema!("tests/fixtures/rpc_batch_null_encoding.cstack", db = None);

/// CBOR simple-value markers this test asserts on directly (RFC 8949 §3.3).
const CBOR_NULL: u8 = 0xf6;
const CBOR_EMPTY_ARRAY: u8 = 0x80;

#[derive(Clone, Default)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    fn echo_note(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::echo_note::Args,
        _authorized: cratestack_schema::procedures::echo_note::Authorized,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::echo_note::Output, CratestackError>,
    > + Send {
        async move {
            Ok(cratestack_schema::EchoReply {
                note: args.args.note,
            })
        }
    }
}

#[derive(Clone, Default)]
struct AlwaysAuthProvider;

impl cratestack::AuthProvider for AlwaysAuthProvider {
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

fn build_router() -> cratestack::axum::Router {
    let db = cratestack_schema::Cratestack::builder().build();
    cratestack_schema::axum::rpc_router(
        db,
        Procedures,
        CborCodec,
        AlwaysAuthProvider,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

#[tokio::test]
async fn batch_null_survives_the_wire_as_cbor_null_both_directions() {
    let router = build_router();

    // Request side: the frame's `input` carries a real JSON `null` for
    // `EchoArgs.note` — exactly the opaque `serde_json::Value` shape
    // `RpcRequest.input` uses (`cratestack-core::rpc`), and exactly what
    // `rpc_batch_dispatch` re-encodes internally before redispatching into
    // `rpc_dispatch_inner`.
    let frames = vec![RpcRequest {
        id: 1,
        op: "procedure.echoNote".into(),
        input: serde_json::json!({ "args": { "note": null } }),
        idem: None,
    }];
    let raw_request_body = CborCodec.encode(&frames).expect("batch body should encode");

    // Decisive assertion #1 (request side): the literal bytes this test
    // sends over HTTP must contain the CBOR null marker for that `note`
    // field, never the empty-array marker. Before the fix, encoding a
    // `Vec<RpcRequest>` whose `input` contains `Value::Null` went through
    // the exact same `CborCodec::encode` codepath `rpc_batch_dispatch`
    // uses internally for `frame.input`, so this reproduces that half of
    // the bug directly on the wire bytes, not just via the isolated codec
    // unit test.
    assert!(
        raw_request_body.contains(&CBOR_NULL),
        "request wire bytes should contain the CBOR null marker 0xf6: {raw_request_body:02x?}",
    );
    assert!(
        !raw_request_body.contains(&CBOR_EMPTY_ARRAY),
        "request wire bytes must not contain the CBOR empty-array marker \
         0x80 (cratestack#657): {raw_request_body:02x?}",
    );

    let response = router
        .oneshot(
            Request::post(RPC_BATCH_PATH)
                .header("content-type", CborCodec::CONTENT_TYPE)
                .header("accept", CborCodec::CONTENT_TYPE)
                .body(Body::from(raw_request_body))
                .expect("request should build"),
        )
        .await
        .expect("batch dispatch should complete");

    assert_eq!(response.status(), StatusCode::OK);
    let raw_response_body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should buffer")
        .to_vec();

    // Decisive assertion #2 (response side): the actual HTTP response body
    // this test receives — genuine wire bytes, no test-side re-encoding
    // involved — must carry the frame's echoed-back `note: null` as CBOR
    // null, not an empty array. This is exactly `response_to_frame` /
    // `encode_transport_result_with_status_for`'s output
    // (`crates/cratestack-axum/src/rpc/batch.rs`).
    assert!(
        raw_response_body.contains(&CBOR_NULL),
        "response wire bytes should contain the CBOR null marker 0xf6: {raw_response_body:02x?}",
    );
    assert!(
        !raw_response_body.contains(&CBOR_EMPTY_ARRAY),
        "response wire bytes must not contain the CBOR empty-array marker \
         0x80 (cratestack#657): {raw_response_body:02x?}",
    );

    // Sanity: the frame round-tripped as a real success, echoing the null
    // back rather than tripping a decode error along the way (which is
    // what a corrupted-to-`0x80` request input would have caused for this
    // `Option<String>` field before the fix).
    let results: Vec<RpcResponseFrame> = CborCodec
        .decode(&raw_response_body)
        .expect("batch response should decode");
    assert_eq!(results.len(), 1);
    let frame = &results[0];
    assert!(
        frame.error.is_none(),
        "echoNote frame should succeed, not error: {frame:?}",
    );
    assert_eq!(
        frame.output,
        Some(serde_json::json!({ "note": null })),
        "echoed output should decode back to a real JSON null for `note`",
    );
}
