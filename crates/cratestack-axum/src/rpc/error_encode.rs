//! Wire-side error encoding: dispatcher errors and handler-emitted error
//! responses both end up as [`RpcErrorBody`] frames.

use axum::http::HeaderMap;
use cratestack_core::CoolError;
use cratestack_core::rpc::RpcErrorBody;
use serde::Serialize;

use crate::HttpTransport;

use super::RPC_BINDING_CAPABILITIES;
use super::codec_helpers::decode_rpc_body;
use super::util::synthesize_error_for_status;

/// Build an `axum::Response` carrying an [`RpcErrorBody`] for a
/// [`CoolError`] raised inside the dispatcher (e.g. body decode
/// failure, unknown op id). The HTTP status comes from
/// [`CoolError::status_code`]; the body is codec-encoded via the
/// request's codec, content-type negotiated against
/// [`RPC_BINDING_CAPABILITIES`].
pub fn encode_rpc_error<C>(
    codec: &C,
    headers: &HeaderMap,
    error: &CoolError,
) -> axum::response::Response
where
    C: HttpTransport,
{
    let body = RpcErrorBody::from_cool(error);
    let status = error.status_code();
    encode_rpc_value_response(codec, headers, status, body)
}

/// Post-process a handler-emitted response. Success responses pass
/// through unchanged. Non-2xx responses are buffered, their bodies
/// decoded as [`cratestack_core::CoolErrorResponse`] (the REST shape
/// the existing axum handlers emit), translated to [`RpcErrorBody`]
/// with the gRPC-style code, and re-encoded with the same HTTP status.
///
/// Called once per dispatch (inside `rpc_dispatch_inner`) so unary and
/// batch both see uniformly RpcErrorBody-shaped error bodies.
pub async fn convert_handler_error_response<C>(
    response: axum::response::Response,
    codec: &C,
    headers: &HeaderMap,
) -> axum::response::Response
where
    C: HttpTransport,
{
    if response.status().is_success() {
        return response;
    }

    let status = response.status();
    let body_bytes = match axum::body::to_bytes(response.into_body(), usize::MAX).await {
        Ok(bytes) => bytes.to_vec(),
        Err(error) => {
            // Buffering failed — synthesize an internal error frame.
            let cool = CoolError::Internal(format!("buffer handler error body: {error}"));
            return encode_rpc_error(codec, headers, &cool);
        }
    };

    let rpc_body =
        match decode_rpc_body::<_, cratestack_core::CoolErrorResponse>(codec, headers, &body_bytes)
        {
            Ok(parsed) => RpcErrorBody::from_cool_response(parsed),
            Err(_) => {
                // Handler emitted a non-2xx with a body that isn't the
                // framework's REST error shape (unusual — would happen if a
                // handler escaped through `into_response()` directly). Build
                // a synthetic body from the status alone.
                let cool = synthesize_error_for_status(status);
                RpcErrorBody::from_cool(&cool)
            }
        };

    encode_rpc_value_response(codec, headers, status, rpc_body)
}

fn encode_rpc_value_response<C, T>(
    codec: &C,
    headers: &HeaderMap,
    status: axum::http::StatusCode,
    value: T,
) -> axum::response::Response
where
    C: HttpTransport,
    T: Serialize,
{
    // Re-use the existing transport encoder so content negotiation
    // happens via the same path as everything else.
    crate::encode_transport_result_with_status_for::<_, T>(
        codec,
        headers,
        &RPC_BINDING_CAPABILITIES,
        status,
        Ok(value),
    )
}
