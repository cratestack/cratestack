//! Per-frame conversion for the batch path.

use axum::http::HeaderMap;
use cratestack_core::CratestackError;
use cratestack_core::rpc::{RpcErrorBody, RpcFrameValue, RpcResponseFrame};

use crate::HttpTransport;

use super::codec_helpers::decode_rpc_body;
use super::util::synthesize_error_for_status;

/// Convert an [`axum::Response`] returned by an inner dispatch arm into a
/// single batch response frame.
///
/// Success bodies (2xx) are decoded as an [`RpcFrameValue`] via the same
/// codec the request used and become `RpcResponseFrame::ok`. Error
/// bodies (4xx/5xx) — which have already been post-processed by
/// [`super::convert_handler_error_response`] inside `rpc_dispatch_inner` —
/// are decoded as [`RpcErrorBody`] and inlined into
/// `RpcResponseFrame::error` directly.
///
/// An [`RpcFrameValue`] keeps a CBOR byte string (a `Uuid`, a `Bytes`
/// field) as bytes, so an output carrying one survives the round trip
/// through the frame (cratestack#1352; a `serde_json::Value` here used to
/// turn such a frame into an error). Limits that remain: map keys must be
/// strings, CBOR tags other than bignums are refused, and `f16`/`f32`
/// widen to `f64`.
pub async fn response_to_frame<C>(
    id: u64,
    response: axum::response::Response,
    codec: &C,
    headers: &HeaderMap,
) -> RpcResponseFrame
where
    C: HttpTransport,
{
    let status = response.status();
    let body_bytes = match axum::body::to_bytes(
        response.into_body(),
        cratestack_core::MAX_RESPONSE_REBUFFER_BYTES,
    )
    .await
    {
        Ok(bytes) => bytes.to_vec(),
        Err(error) => {
            return RpcResponseFrame::err(
                id,
                &CratestackError::Internal(format!("buffer batch frame body: {error}")),
            );
        }
    };

    if status.is_success() {
        match decode_rpc_body::<_, RpcFrameValue>(codec, headers, &body_bytes) {
            Ok(value) => RpcResponseFrame::ok(id, value),
            Err(error) => RpcResponseFrame::err(id, &error),
        }
    } else {
        // Body is already RpcErrorBody-shaped — `rpc_dispatch_inner`
        // post-processes handler errors before they reach us.
        match decode_rpc_body::<_, RpcErrorBody>(codec, headers, &body_bytes) {
            Ok(body) => RpcResponseFrame {
                id,
                output: None,
                error: Some(body),
            },
            Err(_) => {
                // Defensive: a handler/dispatcher returned a non-2xx
                // body that isn't RpcErrorBody-shaped. Synthesize one
                // from the status alone rather than corrupting the
                // batch envelope.
                let synthetic = synthesize_error_for_status(status);
                RpcResponseFrame::err(id, &synthetic)
            }
        }
    }
}
