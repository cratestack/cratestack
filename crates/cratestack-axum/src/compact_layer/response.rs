//! Compacting a `2xx` response.

use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::Response;
use cratestack_codec_cbor::compact::cbor_to_compact;
use cratestack_codec_json::compact::json_to_compact;
use cratestack_core::compact::{
    COMPACT_CBOR_SEQ, COMPACT_JSON, Direction, OpKeys, compact_media_type_for,
};
use cratestack_core::{CratestackError, MAX_RESPONSE_REBUFFER_BYTES};

use super::layer::CompactLayer;
use super::negotiate::response_base;
use super::seq;
use crate::middleware_error::middleware_error_response;

/// `answer` in compact keys when it is a `2xx` in a plain type whose
/// compact twin is in `wanted`; otherwise `answer` as it is.
pub(super) async fn compact(
    layer: &CompactLayer,
    keys: &OpKeys,
    wanted: &[&'static str],
    request_headers: &HeaderMap,
    path: &str,
    answer: Response,
) -> Response {
    if !answer.status().is_success() {
        return answer;
    }
    let Some(target) = response_base(answer.headers())
        .as_deref()
        .and_then(compact_media_type_for)
        .filter(|target| wanted.contains(target))
    else {
        return answer;
    };
    let node = layer.tables.node(keys, Direction::Response).clone();
    let (mut parts, body) = answer.into_parts();
    let body = if target == COMPACT_CBOR_SEQ {
        seq::compact_sequence(body, layer.tables.clone(), node)
    } else {
        let Ok(bytes) = axum::body::to_bytes(body, MAX_RESPONSE_REBUFFER_BYTES).await else {
            return failed(request_headers, path, "read the response body");
        };
        let compacted = match (bytes.is_empty(), target == COMPACT_JSON) {
            (true, _) => Ok(Vec::new()),
            (false, true) => json_to_compact(&layer.tables, &node, &bytes),
            (false, false) => cbor_to_compact(&layer.tables, &node, &bytes),
        };
        let Ok(compacted) = compacted else {
            return failed(request_headers, path, "compact the response body");
        };
        parts
            .headers
            .insert(header::CONTENT_LENGTH, HeaderValue::from(compacted.len()));
        Body::from(Bytes::from(compacted))
    };
    if target == COMPACT_CBOR_SEQ {
        parts.headers.remove(header::CONTENT_LENGTH);
    }
    parts
        .headers
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(target));
    parts.headers.append(
        header::VARY,
        HeaderValue::from_static("Accept, Cratestack-Contract"),
    );
    Response::from_parts(parts, body)
}

fn failed(headers: &HeaderMap, path: &str, what: &str) -> Response {
    tracing::error!(target: "cratestack", cratestack_operation = "compact", "could not {what}");
    middleware_error_response(
        headers,
        path,
        CratestackError::Internal(format!("could not {what}")),
    )
}
