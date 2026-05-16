//! Codec helpers used by the macro-emitted dispatcher: decode the request
//! body, re-encode a typed value.

use axum::http::HeaderMap;
use cratestack_core::CoolError;
use serde::{Deserialize, Serialize};

use crate::HttpTransport;

pub(super) const DEFAULT_CONTENT_TYPE: &str = "application/cbor";

/// Decode an RPC unary request body into `T`, picking the codec based on
/// the request's `Content-Type` header. Missing header → CBOR (the
/// default for the REST binding too).
///
/// Used by the macro-generated RPC dispatcher; safe to use directly.
//
// TODO: this is nearly identical to `decode_transport_request_for` but
// differs in the missing-Content-Type fallback — this helper defaults to
// CBOR, while `decode_transport_request_for` errors with
// `UnsupportedMediaType`. Reconciling the two would change RPC behavior,
// so the bodies are kept distinct for now.
pub fn decode_rpc_body<C, T>(codec: &C, headers: &HeaderMap, body: &[u8]) -> Result<T, CoolError>
where
    C: HttpTransport,
    T: for<'de> Deserialize<'de>,
{
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or(DEFAULT_CONTENT_TYPE);
    codec.decode_request(content_type, body)
}

/// Encode an arbitrary serializable value back to bytes using the same
/// codec as the request. Used by the macro-generated `update` dispatch
/// arm to re-encode the typed patch before handing it to the existing
/// update handler as `Bytes`.
///
/// Async because the codec's `encode_response` returns an `axum::Response`
/// whose body has to be buffered out — in practice the codec always
/// produces an in-memory `Full<Bytes>` body, so this completes in one
/// poll, but we don't depend on that.
pub async fn encode_rpc_value<C, T>(
    codec: &C,
    headers: &HeaderMap,
    value: &T,
) -> Result<Vec<u8>, CoolError>
where
    C: HttpTransport,
    T: Serialize + ?Sized,
{
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or(DEFAULT_CONTENT_TYPE);
    let response = codec.encode_response(content_type, axum::http::StatusCode::OK, value)?;
    let (_parts, body) = response.into_parts();
    let bytes = axum::body::to_bytes(body, usize::MAX)
        .await
        .map_err(|error| {
            CoolError::Internal(format!("failed to buffer encoded RPC body: {error}"))
        })?;
    Ok(bytes.to_vec())
}
