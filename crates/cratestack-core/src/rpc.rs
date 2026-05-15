//! RPC binding wire types.
//!
//! Both the server binding (`cratestack-axum::rpc`) and every
//! generated client (`cratestack-client-rust`, the TS / Dart
//! generators) agree on these shapes. They live in `cratestack-core`
//! so clients can depend on a single source of truth without pulling
//! in axum.
//!
//! Server-only helpers (codec-aware encoding, axum response
//! post-processing, batch frame assembly) stay in
//! `cratestack-axum::rpc`. This module owns only the wire shapes and
//! the [`CoolError`] → gRPC-style code mapping.

use serde::{Deserialize, Serialize};

use crate::error::{CoolError, CoolErrorResponse};

/// Mount path for unary RPC calls. The trailing segment is the
/// percent-decoded op id, e.g. `POST /rpc/model.User.list`.
pub const RPC_UNARY_PATH: &str = "/rpc/{op_id}";

/// Mount path for batched RPC calls. Body is a codec-encoded sequence
/// of [`RpcRequest`] frames.
pub const RPC_BATCH_PATH: &str = "/rpc/batch";

/// Wire shape of a single error returned by an RPC call. Maps from
/// [`CoolError`] via [`rpc_code`] + [`CoolError::public_message`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcErrorBody {
    /// Stable gRPC-style code: `not_found`, `invalid_argument`,
    /// `permission_denied`, `failed_precondition`, `conflict`,
    /// `unauthenticated`, `internal`.
    pub code: String,
    /// Public, safe-to-expose message.
    pub message: String,
    /// Op-defined structured payload (e.g. validation issues).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl RpcErrorBody {
    pub fn from_cool(error: &CoolError) -> Self {
        Self {
            code: rpc_code(error).to_owned(),
            message: error.public_message().into_owned(),
            details: None,
        }
    }

    /// Translate a REST-style [`CoolErrorResponse`] into the RPC
    /// error body. The `code` field is mapped from screaming-snake to
    /// gRPC-style lowercase via [`cool_error_code_to_rpc_code`];
    /// `message` and `details` flow through verbatim.
    pub fn from_cool_response(response: CoolErrorResponse) -> Self {
        let CoolErrorResponse {
            code,
            message,
            details,
        } = response;
        Self {
            code: cool_error_code_to_rpc_code(&code).to_owned(),
            message,
            details: details.map(cool_value_to_json),
        }
    }
}

/// Wire shape of a single batch request frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    /// Client-chosen correlation id, unique within the batch.
    pub id: u64,
    /// Dotted op id, e.g. `"model.User.list"` or
    /// `"procedure.publishPost"`.
    pub op: String,
    /// Codec-encoded input payload, kept opaque at the batch envelope
    /// layer so each frame can be decoded against its own input type.
    pub input: serde_json::Value,
    /// Optional idempotency key, per-frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idem: Option<String>,
}

/// Wire shape of a single batch response frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponseFrame {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcErrorBody>,
}

impl RpcResponseFrame {
    pub fn ok(id: u64, output: serde_json::Value) -> Self {
        Self {
            id,
            output: Some(output),
            error: None,
        }
    }

    pub fn err(id: u64, error: &CoolError) -> Self {
        Self {
            id,
            output: None,
            error: Some(RpcErrorBody::from_cool(error)),
        }
    }
}

/// Map a [`CoolError`] to its stable RPC code (gRPC-style snake_case).
pub const fn rpc_code(error: &CoolError) -> &'static str {
    match error {
        CoolError::BadRequest(_)
        | CoolError::NotAcceptable(_)
        | CoolError::UnsupportedMediaType(_)
        | CoolError::Codec(_)
        | CoolError::Validation(_) => "invalid_argument",
        CoolError::Unauthorized(_) => "unauthenticated",
        CoolError::Forbidden(_) => "permission_denied",
        CoolError::NotFound(_) => "not_found",
        CoolError::Conflict(_) => "conflict",
        CoolError::PreconditionFailed(_) => "failed_precondition",
        CoolError::Database(_) | CoolError::Internal(_) => "internal",
    }
}

/// Map a `CoolErrorResponse.code` string (screaming-snake, REST-
/// binding vocabulary) to the stable gRPC-style code the RPC binding
/// emits.
pub fn cool_error_code_to_rpc_code(code: &str) -> &'static str {
    match code {
        "BAD_REQUEST"
        | "NOT_ACCEPTABLE"
        | "UNSUPPORTED_MEDIA_TYPE"
        | "VALIDATION_ERROR"
        | "CODEC_ERROR" => "invalid_argument",
        "UNAUTHORIZED" => "unauthenticated",
        "FORBIDDEN" => "permission_denied",
        "NOT_FOUND" => "not_found",
        "CONFLICT" => "conflict",
        "PRECONDITION_FAILED" => "failed_precondition",
        "DATABASE_ERROR" | "INTERNAL_ERROR" => "internal",
        _ => "internal",
    }
}

fn cool_value_to_json(value: crate::Value) -> serde_json::Value {
    serde_json::to_value(&value).unwrap_or(serde_json::Value::Null)
}
