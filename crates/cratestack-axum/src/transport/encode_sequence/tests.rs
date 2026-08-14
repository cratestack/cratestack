//! cratestack#489 coverage for the sequence/`@stream` encode paths:
//! response content-type negotiation must stay codec-aware here too, and
//! `application/cbor-seq` behavior (including the incremental
//! `encode_transport_stream_result_with_status_for` path) must be exactly
//! unchanged when the router genuinely does have a CBOR codec.

use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use cratestack_core::CratestackError;
use futures_util::stream;
use serde::Serialize;

use super::*;
use crate::codec::CodecSet;

#[derive(Serialize, Clone)]
struct Item {
    n: u32,
}

fn stream_capabilities() -> RouteTransportCapabilities {
    RouteTransportCapabilities {
        request_types: &["application/cbor", "application/json"],
        response_types: &[
            "application/cbor",
            "application/json",
            CBOR_SEQUENCE_CONTENT_TYPE,
        ],
        default_response_type: "application/cbor",
        supports_sequence_response: true,
    }
}

fn headers_with_accept(value: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::ACCEPT, HeaderValue::from_str(value).unwrap());
    headers
}

/// The buffered (non-`@stream`) sequence path must apply the same
/// codec-aware negotiation as the unary path.
#[test]
fn json_only_router_serves_a_list_response_even_when_accept_also_lists_cbor() {
    let headers = headers_with_accept("application/json, application/cbor");
    let response = encode_transport_sequence_result_with_status_for(
        &JsonCodec,
        &headers,
        &stream_capabilities(),
        StatusCode::OK,
        Ok::<_, CratestackError>(vec![Item { n: 1 }, Item { n: 2 }]),
    );
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("application/json"),
    );
}

/// `@stream` behavior for a router that genuinely has a CBOR codec must
/// be unchanged by cratestack#489: `Accept: application/cbor-seq` still
/// gets the truly incremental encoder.
#[tokio::test]
async fn stream_negotiates_cbor_seq_unchanged_when_a_cbor_codec_is_present() {
    let codec = CodecSet::new(CborCodec, JsonCodec);
    let headers = headers_with_accept(CBOR_SEQUENCE_CONTENT_TYPE);
    let items = stream::iter(vec![
        Ok::<_, CratestackError>(Item { n: 1 }),
        Ok(Item { n: 2 }),
    ]);
    let response = encode_transport_stream_result_with_status_for(
        &codec,
        &headers,
        &stream_capabilities(),
        StatusCode::OK,
        Ok(items),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some(CBOR_SEQUENCE_CONTENT_TYPE),
    );
}

/// A `JsonCodec`-only router has no CBOR codec at all, so `cbor-seq` was
/// never satisfiable and still isn't — this must keep 406ing, not start
/// silently downgrading to a JSON array (that would be a real streaming
/// semantics change, not just a content-type fix).
#[tokio::test]
async fn stream_still_406s_for_cbor_seq_on_a_json_only_router() {
    let headers = headers_with_accept(CBOR_SEQUENCE_CONTENT_TYPE);
    let items = stream::iter(vec![Ok::<_, CratestackError>(Item { n: 1 })]);
    let response = encode_transport_stream_result_with_status_for(
        &JsonCodec,
        &headers,
        &stream_capabilities(),
        StatusCode::OK,
        Ok(items),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_ACCEPTABLE);
}

/// A `@stream` procedure requested with a plain (non-cbor-seq) `Accept`
/// still falls back to the buffered array encoder, same as before this
/// fix — and still correctly avoids the codec the router doesn't have.
#[tokio::test]
async fn stream_falls_back_to_buffered_json_for_a_json_only_router_with_a_plain_accept() {
    let headers = headers_with_accept("application/json, application/cbor");
    let items = stream::iter(vec![
        Ok::<_, CratestackError>(Item { n: 1 }),
        Ok(Item { n: 2 }),
    ]);
    let response = encode_transport_stream_result_with_status_for(
        &JsonCodec,
        &headers,
        &stream_capabilities(),
        StatusCode::OK,
        Ok(items),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("application/json"),
    );
}
