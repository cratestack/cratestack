//! Decoding here goes through `CratestackCodec::decode` on the very same
//! `CborCodec`/`JsonCodec` a generated client uses:
//! `cratestack_client_rust::HttpClientCodec::decode_response` matches the
//! content type and then calls exactly this method. Asserting on the
//! decoded struct — not on the raw bytes — is the point: the bug this
//! module fixes was that the body did not decode into the error shape at
//! all.

use axum::body::to_bytes;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use cratestack_core::rpc::RpcErrorBody;
use cratestack_core::{CratestackCodec, CratestackError, CratestackErrorResponse};

use super::middleware_error_response;

fn headers_with_accept(accept: Option<&str>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Some(accept) = accept {
        headers.insert(header::ACCEPT, HeaderValue::from_str(accept).unwrap());
    }
    headers
}

async fn parts(response: Response) -> (StatusCode, String, Vec<u8>) {
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let bytes = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body should buffer");
    (status, content_type, bytes.to_vec())
}

#[tokio::test]
async fn rest_path_emits_the_rest_envelope_in_cbor_by_default() {
    let response = middleware_error_response(
        &headers_with_accept(None),
        "/transfer",
        CratestackError::TooManyRequests("rate limit exceeded".to_owned()),
    );
    let (status, content_type, body) = parts(response).await;

    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(content_type, "application/cbor");
    let decoded: CratestackErrorResponse = CborCodec
        .decode(&body)
        .expect("a middleware error body must decode as the framework error envelope");
    assert_eq!(decoded.code, "TOO_MANY_REQUESTS");
    assert_eq!(decoded.message, "rate limit exceeded");
}

#[tokio::test]
async fn accept_json_is_honoured_for_the_error_body_too() {
    let response = middleware_error_response(
        &headers_with_accept(Some("application/json")),
        "/transfer",
        CratestackError::PreconditionFailed("no verifiable caller identity".to_owned()),
    );
    let (status, content_type, body) = parts(response).await;

    assert_eq!(status, StatusCode::PRECONDITION_FAILED);
    assert_eq!(content_type, "application/json");
    let decoded: CratestackErrorResponse = JsonCodec.decode(&body).expect("json error envelope");
    assert_eq!(decoded.code, "PRECONDITION_FAILED");
}

#[tokio::test]
async fn rpc_path_emits_the_rpc_envelope_with_the_grpc_style_code() {
    let response = middleware_error_response(
        &headers_with_accept(None),
        "/rpc/procedure.transfer",
        CratestackError::TooManyRequests("rate limit exceeded".to_owned()),
    );
    let (status, content_type, body) = parts(response).await;

    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(content_type, "application/cbor");
    let decoded: RpcErrorBody = CborCodec
        .decode(&body)
        .expect("an RPC middleware error body must decode as RpcErrorBody");
    assert_eq!(
        decoded.code, "resource_exhausted",
        "the RPC binding's vocabulary is gRPC-style lowercase, not the REST screaming-snake code"
    );
}

/// The RPC router is routinely mounted under an application prefix, so
/// the transport probe must not be anchored at the start of the path.
#[tokio::test]
async fn nested_rpc_mount_still_gets_the_rpc_envelope() {
    let response = middleware_error_response(
        &headers_with_accept(None),
        "/api/v1/rpc/model.Account.list",
        CratestackError::Internal("redis rate limit: broken pipe".to_owned()),
    );
    let (_, _, body) = parts(response).await;
    let decoded: RpcErrorBody = CborCodec.decode(&body).expect("rpc envelope");
    assert_eq!(decoded.code, "internal");
    assert_eq!(
        decoded.message, "internal error",
        "5xx detail must stay operator-only, exactly as the handler path redacts it"
    );
}

/// A REST path that merely *mentions* rpc without a `/rpc/` segment must
/// keep the REST vocabulary — the probe is a segment test, not a search
/// for the three letters.
#[tokio::test]
async fn rest_path_mentioning_rpc_without_a_segment_stays_rest() {
    let response = middleware_error_response(
        &headers_with_accept(None),
        "/rpcs/transfer",
        CratestackError::Internal("boom".to_owned()),
    );
    let (_, _, body) = parts(response).await;
    let decoded: CratestackErrorResponse = CborCodec.decode(&body).expect("rest envelope");
    assert_eq!(decoded.code, "INTERNAL_ERROR");
}
