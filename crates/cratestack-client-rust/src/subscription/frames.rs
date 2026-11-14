//! A server frame's item or error, the same on every binding.

use cratestack_core::CratestackErrorResponse;
use cratestack_core::rpc::{RpcErrorBody, rpc_code_to_cratestack_error_code};
use reqwest::StatusCode;
use serde::de::DeserializeOwned;

use crate::error::ClientError;
use crate::remote_refusal::Challenges;

/// An `err` frame as a `ClientError::Remote`, with the HTTP status its code
/// stands for, so it reads like a refusal of a unary call.
pub fn rpc_error_to_client(body: RpcErrorBody) -> ClientError {
    let status = match body.code.as_str() {
        "invalid_argument" => StatusCode::BAD_REQUEST,
        "unauthenticated" => StatusCode::UNAUTHORIZED,
        "permission_denied" => StatusCode::FORBIDDEN,
        "not_found" => StatusCode::NOT_FOUND,
        "conflict" | "aborted" => StatusCode::CONFLICT,
        "failed_precondition" => StatusCode::PRECONDITION_FAILED,
        "resource_exhausted" => StatusCode::TOO_MANY_REQUESTS,
        "unavailable" => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    let message = body.message.clone();
    // The same members as the REST envelope (`code`, `message`, `details`,
    // the extras), so the round trip loses nothing.
    // The envelope speaks the REST code names, so the RPC code is translated
    // on the way in; `RpcErrorBody::from_cratestack_response` translates it
    // back, and the code a caller reads is the one the server sent.
    let error = serde_json::to_value(&body)
        .map(|mut value| {
            value["code"] = rpc_code_to_cratestack_error_code(&body.code).into();
            value
        })
        .and_then(serde_json::from_value::<CratestackErrorResponse>)
        .ok()
        .map(Box::new);
    ClientError::Remote {
        status,
        error,
        message,
        challenges: Challenges::default(),
    }
}

/// An item frame's value as `T`.
pub(super) fn item<T: DeserializeOwned>(value: serde_json::Value) -> Result<T, ClientError> {
    serde_json::from_value(value)
        .map_err(|error| ClientError::InvalidResponse(format!("subscription item: {error}")))
}

/// A socket frame's item as `T`, the way a unary CBOR call decodes: a `Uuid`
/// is a byte string and `Bytes` stay bytes (cratestack#1360).
///
/// A server from before that fix sent a `Uuid` as text on these sockets, which
/// the CBOR rules refuse, so on failure the item is read once more as the
/// human-readable JSON value it was then. If both fail, the first error is
/// the one reported. WebSocket and WebTransport only: SSE is JSON.
#[cfg(all(
    any(feature = "ws", feature = "webtransport"),
    not(target_arch = "wasm32")
))]
pub(super) fn cbor_item<T: DeserializeOwned>(
    value: cratestack_core::rpc::RpcFrameValue,
) -> Result<T, ClientError> {
    let first = match value.decode_with::<_, T>(&cratestack_codec_cbor::CborCodec) {
        Ok(item) => return Ok(item),
        Err(error) => error,
    };
    serde_json::from_value(serde_json::Value::from(value))
        .map_err(|_| ClientError::InvalidResponse(format!("subscription item: {first}")))
}

#[cfg(test)]
mod tests;
