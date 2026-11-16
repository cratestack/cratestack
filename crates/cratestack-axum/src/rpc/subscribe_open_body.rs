//! The input of `POST /rpc/subscribe/{op_id}`, decoded by `Content-Type`
//! (cratestack#1001): `application/cbor` is a CBOR document, anything else —
//! `application/json`, or no header, as before — is JSON. Either way it
//! becomes the JSON value a dispatcher turns into the feed's query string, so
//! a CBOR byte string (a `Uuid`) reads as the array a JSON `Bytes` would.

use axum::http::{HeaderMap, header};
use cratestack_codec_cbor::CborCodec;
use cratestack_core::rpc::RpcFrameValue;
use cratestack_core::{CratestackCodec, CratestackError};

/// An empty body is `null`: a subscription with no input.
pub fn decode_subscribe_open_body(
    headers: &HeaderMap,
    body: &[u8],
) -> Result<serde_json::Value, CratestackError> {
    if body.is_empty() {
        return Ok(serde_json::Value::Null);
    }
    let is_cbor = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|content_type| {
            content_type
                .split(';')
                .next()
                .is_some_and(|media| media.trim().eq_ignore_ascii_case("application/cbor"))
        });
    if is_cbor {
        let frame: RpcFrameValue = CborCodec.decode(body).map_err(|error| {
            CratestackError::BadRequest(format!("a subscription's body is its CBOR input: {error}"))
        })?;
        return Ok(frame.into());
    }
    serde_json::from_slice(body).map_err(|error| {
        CratestackError::BadRequest(format!("a subscription's body is its JSON input: {error}"))
    })
}

#[cfg(test)]
mod tests;
