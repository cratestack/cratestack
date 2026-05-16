//! Per-frame conversion for the batch path.

use axum::http::HeaderMap;
use cratestack_core::CoolError;
use cratestack_core::rpc::{RpcErrorBody, RpcResponseFrame};

use crate::HttpTransport;

use super::codec_helpers::decode_rpc_body;
use super::util::synthesize_error_for_status;

/// Convert an [`axum::Response`] returned by an inner dispatch arm into a
/// single batch response frame.
///
/// Success bodies (2xx) are decoded as `serde_json::Value` via the same
/// codec the request used and become `RpcResponseFrame::ok`. Error
/// bodies (4xx/5xx) — which have already been post-processed by
/// [`super::convert_handler_error_response`] inside `rpc_dispatch_inner` —
/// are decoded as [`RpcErrorBody`] and inlined into
/// `RpcResponseFrame::error` directly.
///
/// Wire limitation: success outputs must be representable as
/// `serde_json::Value`. For CRUD/procedure outputs this is fine; if a
/// future op returns CBOR-only types (e.g. raw byte strings without a
/// JSON representation) the frame becomes an `internal` error.
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
    let body_bytes = match axum::body::to_bytes(response.into_body(), usize::MAX).await {
        Ok(bytes) => bytes.to_vec(),
        Err(error) => {
            return RpcResponseFrame::err(
                id,
                &CoolError::Internal(format!("buffer batch frame body: {error}")),
            );
        }
    };

    if status.is_success() {
        match decode_rpc_body::<_, serde_json::Value>(codec, headers, &body_bytes) {
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
