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

/// Mount path for `@@subscribe` SSE subscriptions
/// (`docs/design/rpc-transport.md` §3.4a, cratestack#390). The trailing
/// segment is the percent-decoded op id, e.g.
/// `GET /rpc/subscribe/model.User.subscribe`. Unlike [`RPC_UNARY_PATH`]
/// this is `GET`-only and carries no request body — auth is header-based
/// (same as every other HTTP RPC binding), not an upgrade-time HMAC like
/// the WS path (§3.4).
pub const RPC_SUBSCRIBE_PATH: &str = "/rpc/subscribe/{op_id}";

/// CBOR tag number reserved for the mid-stream error sentinel described
/// in `docs/design/rpc-transport.md` §3.3: when a genuinely incremental
/// `application/cbor-seq` sequence response (a `@stream` procedure, see
/// cratestack#282/#283) fails partway through, the *last* item of the
/// sequence is `Tag(RPC_STREAM_ERROR_TAG, RpcErrorBody-as-CBOR-map)` —
/// CBOR major type 6, this tag number, wrapping [`RpcErrorBody`] encoded
/// as a plain CBOR map — in place of what would otherwise be the next
/// unwrapped `out` item. No further items follow it; end of body comes
/// immediately after.
///
/// Not IANA-registered. Picked from the CBOR tags registry's "First Come
/// First Served" range (32768–18446744073709551615;
/// <https://www.iana.org/assignments/cbor-tags/cbor-tags.xhtml>) and
/// confirmed unassigned as of 2026-08-02 — see cratestack#281 for the
/// verification method and the collision-risk flag for a pre-merge
/// human double-check.
pub const RPC_STREAM_ERROR_TAG: u64 = 48900;

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
        CoolError::Conflict(_) | CoolError::ConflictTyped(_) => "conflict",
        CoolError::PreconditionFailed(_) => "failed_precondition",
        CoolError::Database(_) | CoolError::DatabaseTyped(_) | CoolError::Internal(_) => "internal",
        CoolError::Unavailable(_) => "unavailable",
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
        "UNAVAILABLE" => "unavailable",
        _ => "internal",
    }
}

fn cool_value_to_json(value: crate::Value) -> serde_json::Value {
    serde_json::to_value(&value).unwrap_or(serde_json::Value::Null)
}

// ---------------------------------------------------------------------------
// RPC model-CRUD input envelopes (cratestack#490).
//
// Moved here from `cratestack-axum::rpc::inputs`, which is where they lived
// until this fix — an oversight relative to this module's own opening
// paragraph ("They live in `cratestack-core` so clients can depend on a
// single source of truth without pulling in axum") and relative to the
// sibling wire shapes above (`RpcErrorBody`/`RpcRequest`/`RpcResponseFrame`),
// which already made that move. It went unnoticed until `cratestack-client`
// (a facade with genuinely no `cratestack-axum` dependency) tried to compile
// a `transport rpc` schema with model CRUD and hit "cannot find
// `RpcListInput` in `rpc`" — every previous consumer of
// `::cratestack::rpc::RpcListInput` (`cratestack-pg`, `cratestack-api`,
// `cratestack-sqlite`) carries `cratestack-axum` regardless, so the wrong
// source crate was invisible until a facade without it existed to surface
// it. `cratestack-axum::rpc` re-exports these same three types verbatim
// (`pub use cratestack_core::rpc::{RpcListInput, RpcListPredicate,
// RpcPkInput, RpcUpdateInput};`), so `cratestack-pg`/`cratestack-api`/
// `cratestack-sqlite` see no behavior change — same names, same shapes, same
// wire format, only the defining crate moved.
// ---------------------------------------------------------------------------

/// RPC input for `model.<X>.get` and `model.<X>.delete`. The PK type is
/// instantiated per-model at the macro emission site.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcPkInput<Pk> {
    pub id: Pk,
}

/// RPC input for `model.<X>.update`. Parameterized on both the PK type
/// and the model's concrete `Update<X>Input` so the patch decodes
/// straight to its real type — round-tripping through
/// `serde_json::Value` would corrupt CBOR `Option::None` values (which
/// `minicbor-serde` encodes as `0xf6` simple-null but `serde_json::Value`
/// encodes as the CBOR empty-array marker; see comments in
/// `cratestack-codec-cbor`). The dispatcher re-encodes `patch` through
/// the same codec before handing it to the existing update handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcUpdateInput<Pk, Patch> {
    pub id: Pk,
    pub patch: Patch,
}

/// Single arbitrary key/value predicate inside [`RpcListInput::filters`].
/// Models the REST URL form's "anything that isn't a reserved keyword is a
/// predicate" rule (e.g. `?published=true&authorId=42`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcListPredicate {
    pub key: String,
    pub value: String,
}

/// RPC input for `model.<X>.list`. Mirrors the REST URL query 1:1 — every
/// optional field maps to a query param of the same name, predicates carry
/// arbitrary `(key, value)` pairs that aren't reserved keywords.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RpcListInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
    /// Selection fields (`?fields=a,b,c`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<String>>,
    /// Included relations (`?include=author,comments`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
    /// Fields per included relation (`?includeFields[author]=id,name`).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub include_fields: std::collections::BTreeMap<String, Vec<String>>,
    /// Order expression (`?sort=name asc`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<String>,
    /// Top-level filter expression (`?where=...`).
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "where")]
    pub where_expr: Option<String>,
    /// Disjunction filter (`?or=...`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub or: Option<String>,
    /// Arbitrary `key=value` predicates (anything not in the reserved set).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<RpcListPredicate>,
}
