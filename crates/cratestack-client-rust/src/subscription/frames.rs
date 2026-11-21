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
/// Two fallbacks, each reached only when the one before it fails; if all
/// fail, the first error is the one reported.
///
/// 1. A `T` holding a `serde_json::Value` (a `ModelEvent<serde_json::Value>`
///    partial-record subscriber) cannot take a byte string, which the server
///    now writes for `Bytes`/`Geography`/`Geometry` (cratestack#1425). The
///    item is read as the JSON value a byte string would have been, an
///    integer array, with the envelope's own `event_id` (a 16-byte `Uuid`)
///    turned back into text, which is what `Uuid` reads from a JSON value.
/// 2. A server from before cratestack#1360 sent a `Uuid` as text on these
///    sockets, which the CBOR rules refuse, so the item is read as the
///    human-readable JSON value it was then.
///
/// Every binding but SSE, which is JSON.
pub(super) fn cbor_item<T: DeserializeOwned>(
    value: cratestack_core::rpc::RpcFrameValue,
) -> Result<T, ClientError> {
    let first = match value.decode_with::<_, T>(&cratestack_codec_cbor::CborCodec) {
        Ok(item) => return Ok(item),
        Err(error) => error,
    };
    serde_json::from_value(json_item(value))
        .map_err(|_| ClientError::InvalidResponse(format!("subscription item: {first}")))
}

/// A frame item as JSON: byte strings become integer arrays, except the
/// envelope `event_id`, a `Uuid`, which becomes its text form.
fn json_item(mut value: cratestack_core::rpc::RpcFrameValue) -> serde_json::Value {
    use cratestack_core::rpc::RpcFrameValue;
    if let RpcFrameValue::Object(fields) = &mut value
        && let Some(RpcFrameValue::Bytes(bytes)) = fields.get("event_id")
        && bytes.len() == 16
    {
        let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        let id = format!(
            "{}-{}-{}-{}-{}",
            &hex[..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..]
        );
        fields.insert("event_id".to_owned(), RpcFrameValue::String(id));
    }
    serde_json::Value::from(value)
}

/// An SSE `message` event's `next` frame as `T`.
pub(super) fn next_of<T: DeserializeOwned>(data: &str) -> Result<T, ClientError> {
    #[derive(serde::Deserialize)]
    struct Next {
        next: serde_json::Value,
    }
    let frame: Next = serde_json::from_str(data)
        .map_err(|error| ClientError::InvalidResponse(format!("subscription frame: {error}")))?;
    item(frame.next)
}

/// An SSE `error` event's `err` frame as a `ClientError`.
pub(super) fn error_of(data: &str) -> ClientError {
    #[derive(serde::Deserialize)]
    struct Err {
        err: RpcErrorBody,
    }
    match serde_json::from_str::<Err>(data) {
        Ok(frame) => rpc_error_to_client(frame.err),
        Err(error) => ClientError::InvalidResponse(format!("subscription error frame: {error}")),
    }
}

#[cfg(test)]
mod tests;
