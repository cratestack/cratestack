//! Runtime primitives for the `transport rpc` generation style.
//!
//! See `docs/design/rpc-transport.md` for the full design. This module
//! provides the binding-side surface that schemas with `transport rpc`
//! generate against:
//!
//! - `POST /rpc/{op_id}` — unary calls. Body is the codec-encoded *input*
//!   (no frame wrapper); response body is the codec-encoded *output* on
//!   success, or an [`RpcErrorBody`] on error with HTTP status mapped via
//!   [`CoolError::status_code`].
//! - `POST /rpc/batch` — sequence of `RpcRequest` frames in, sequence of
//!   `RpcResponseFrame` frames out in the same order. Per-frame errors
//!   don't poison the batch.
//!
//! Subscriptions and streaming live on WebSocket and `application/cbor-seq`
//! respectively; they are deferred to a follow-up patch.
//!
//! The macro emits the dispatch table and the `rpc_router` constructor.
//! This crate provides the shared frame shapes, error mapping, and the
//! `RPC_*_PATH` constants both sides agree on.

use cratestack_core::CoolError;
use serde::{Deserialize, Serialize};

/// Mount path for unary RPC calls. The trailing segment is the
/// percent-decoded op id, e.g. `POST /rpc/model.User.list`.
pub const RPC_UNARY_PATH: &str = "/rpc/{op_id}";

/// Mount path for batched RPC calls. Body is a codec-encoded sequence of
/// [`RpcRequest`] frames.
pub const RPC_BATCH_PATH: &str = "/rpc/batch";

/// Wire shape of a single error returned by an RPC call. Maps from
/// [`CoolError`] via [`rpc_code`] + [`CoolError::public_message`].
///
/// The shape is deliberately tiny (no structured `details` yet) so the
/// surface is forward-compatible: clients written today against
/// `{code, message}` keep working when `details` is added later.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcErrorBody {
    /// Stable gRPC-style code: `not_found`, `invalid_argument`,
    /// `permission_denied`, `failed_precondition`, `conflict`,
    /// `unauthenticated`, `internal`. Never a server-internal enum name.
    pub code: String,
    /// Public, safe-to-expose message. For 5xx errors this is a fixed
    /// canned string; the detailed operator message is logged via
    /// tracing only, never returned over the wire.
    pub message: String,
}

impl RpcErrorBody {
    pub fn from_cool(error: &CoolError) -> Self {
        Self {
            code: rpc_code(error).to_owned(),
            message: error.public_message().into_owned(),
        }
    }
}

/// Wire shape of a single batch request frame.
///
/// Used for [`RPC_BATCH_PATH`] only — unary calls send the input payload
/// unwrapped (the op id is in the URL, the correlation id is irrelevant
/// for one-shot HTTP).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    /// Client-chosen correlation id, unique within the batch. Echoed
    /// back on the matching response frame.
    pub id: u64,
    /// Dotted op id, e.g. `"model.User.list"` or `"procedure.publishPost"`.
    pub op: String,
    /// Codec-encoded input payload, kept opaque at the batch envelope
    /// layer so each frame can be decoded against its own input type.
    pub input: serde_json::Value,
    /// Optional idempotency key, per-frame. The batch route rejects an
    /// `Idempotency-Key` HTTP header as ambiguous; idempotency is always
    /// per-frame in batch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idem: Option<String>,
}

/// Wire shape of a single batch response frame. Tagged by which field is
/// present — `output` on success, `error` on failure — so the variant
/// discriminator is one map key, not a separate `type` field.
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
///
/// CoolError is the framework's internal error representation and uses
/// its own SCREAMING_CASE codes for the REST binding. The RPC binding
/// translates at the wire boundary so clients across both bindings see
/// the same vocabulary they expect for their transport.
///
/// `unavailable`, `deadline_exceeded`, and `canceled` are reserved for
/// future use (rate limit hit, request timeout, client cancellation)
/// and not currently produced by this mapping.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpc_code_maps_each_cool_error_variant() {
        assert_eq!(rpc_code(&CoolError::BadRequest("x".into())), "invalid_argument");
        assert_eq!(rpc_code(&CoolError::NotAcceptable("x".into())), "invalid_argument");
        assert_eq!(rpc_code(&CoolError::Unauthorized("x".into())), "unauthenticated");
        assert_eq!(
            rpc_code(&CoolError::UnsupportedMediaType("x".into())),
            "invalid_argument",
        );
        assert_eq!(rpc_code(&CoolError::Forbidden("x".into())), "permission_denied");
        assert_eq!(rpc_code(&CoolError::NotFound("x".into())), "not_found");
        assert_eq!(rpc_code(&CoolError::Conflict("x".into())), "conflict");
        assert_eq!(rpc_code(&CoolError::Validation("x".into())), "invalid_argument");
        assert_eq!(
            rpc_code(&CoolError::PreconditionFailed("x".into())),
            "failed_precondition",
        );
        assert_eq!(rpc_code(&CoolError::Codec("x".into())), "invalid_argument");
        assert_eq!(rpc_code(&CoolError::Database("x".into())), "internal");
        assert_eq!(rpc_code(&CoolError::Internal("x".into())), "internal");
    }

    #[test]
    fn error_body_uses_public_message_not_operator_detail() {
        // 5xx variants must return the canned public message, never the
        // operator-only detail string carried inside the variant.
        let body = RpcErrorBody::from_cool(&CoolError::Internal("db ip refused".into()));
        assert_eq!(body.code, "internal");
        assert_eq!(body.message, "internal error");
        assert!(
            !body.message.contains("db ip refused"),
            "internal error detail leaked to the wire: {}",
            body.message,
        );
    }

    #[test]
    fn error_body_uses_caller_supplied_message_for_4xx() {
        let body = RpcErrorBody::from_cool(&CoolError::NotFound("widget 42".into()));
        assert_eq!(body.code, "not_found");
        assert_eq!(body.message, "widget 42");
    }

    #[test]
    fn response_frame_ok_and_err_are_mutually_exclusive() {
        let ok = RpcResponseFrame::ok(1, serde_json::json!({"x": 1}));
        assert!(ok.output.is_some());
        assert!(ok.error.is_none());

        let err = RpcResponseFrame::err(2, &CoolError::NotFound("x".into()));
        assert!(err.output.is_none());
        assert!(err.error.is_some());
    }
}
