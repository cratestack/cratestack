//! cratestack#413 — the four `to_bytes(..., MAX_RESPONSE_REBUFFER_BYTES)`
//! response-rebuffer sites degrade to a clean error instead of an
//! unbounded allocation. Each site already matched on `to_bytes`'s
//! `Result` before this change (see each function's own `Err(error) =>`
//! arm), so the assertion here is specifically that a body over the bound
//! produces that existing error path — not a panic — now that the bound is
//! real instead of `usize::MAX`.

#![cfg(test)]

use axum::body::Body;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use cratestack_codec_cbor::CborCodec;
use cratestack_core::{CratestackCodec, MAX_RESPONSE_REBUFFER_BYTES};

use super::{bridge_grpc_response, convert_handler_error_response, response_to_frame};
use crate::rpc::codec_helpers::encode_rpc_value;

fn oversized_body() -> Body {
    Body::from(vec![b'a'; MAX_RESPONSE_REBUFFER_BYTES + 1])
}

#[tokio::test]
async fn response_to_frame_degrades_cleanly_over_the_bound() {
    let response = Response::builder()
        .status(StatusCode::OK)
        .body(oversized_body())
        .unwrap();

    let frame = response_to_frame(1, response, &CborCodec, &HeaderMap::new()).await;

    assert!(frame.output.is_none());
    let error = frame
        .error
        .expect("oversized body should synthesize an error frame");
    assert_eq!(error.code, "internal");
}

#[tokio::test]
async fn convert_handler_error_response_degrades_cleanly_over_the_bound() {
    // Only non-2xx responses reach the `to_bytes` call — see this
    // function's own early return for success responses.
    let response = Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .body(oversized_body())
        .unwrap();

    let converted = convert_handler_error_response(response, &CborCodec, &HeaderMap::new()).await;

    // Buffering failed, so this synthesizes an internal error rather than
    // propagating the handler's original 400.
    assert_eq!(converted.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn bridge_grpc_response_degrades_cleanly_over_the_bound() {
    let response = Response::builder()
        .status(StatusCode::OK)
        .body(oversized_body())
        .unwrap();

    let result: Result<serde_json::Value, _> =
        bridge_grpc_response(response, &CborCodec, &HeaderMap::new()).await;

    let (code, _message) = result.expect_err("oversized body must not succeed silently");
    assert_eq!(code, "INTERNAL_ERROR");
}

#[tokio::test]
async fn encode_rpc_value_degrades_cleanly_over_the_bound() {
    // A value whose *encoded* form lands over the bound — a big plain
    // string is enough regardless of codec framing overhead.
    let huge = "a".repeat(MAX_RESPONSE_REBUFFER_BYTES + 4096);

    let result = encode_rpc_value(&CborCodec, &HeaderMap::new(), &huge).await;

    let error = result.expect_err("oversized encoded value must not succeed silently");
    assert_eq!(error.code(), "INTERNAL_ERROR");
}

#[tokio::test]
async fn response_to_frame_still_succeeds_comfortably_under_the_bound() {
    let encoded = CborCodec
        .encode(&serde_json::json!({ "ok": true }))
        .expect("value should encode");
    let response = Response::builder()
        .status(StatusCode::OK)
        .body(Body::from(encoded))
        .unwrap();

    let frame = response_to_frame(1, response, &CborCodec, &HeaderMap::new()).await;
    assert!(
        frame.error.is_none(),
        "small body should decode cleanly: {frame:?}"
    );
    assert_eq!(
        frame.output.as_ref().and_then(|v| v.get("ok")),
        Some(&serde_json::Value::Bool(true)),
    );
}
