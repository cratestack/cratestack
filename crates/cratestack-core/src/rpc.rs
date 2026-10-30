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
//! the [`CratestackError`] → gRPC-style code mapping.

use serde::{Deserialize, Serialize};

use crate::error::CratestackError;

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

    pub fn err(id: u64, error: &CratestackError) -> Self {
        Self {
            id,
            output: None,
            error: Some(RpcErrorBody::from_cratestack(error)),
        }
    }
}

// RPC model-CRUD input envelopes. Split into their own module rather than
// appended here: this file is the wire-shape module and was already at 180
// lines, and the ~85 lines of input envelopes push it past the ~200-LoC
// ceiling this workspace keeps. They arrived from `cratestack-axum::rpc::
// inputs`, which was itself a dedicated file, so the fine-grained layout is
// preserved rather than flattened. Glob-re-exported so every existing
// `cratestack_core::rpc::RpcListInput` path — and `cratestack-axum::rpc`'s
// verbatim re-export of it — resolves unchanged.
mod inputs;

pub use inputs::*;

// The error body, split out when cratestack#1208 gave it extra members and a
// hand-written serde (see `error_body.rs`).
mod error_body;

pub use error_body::RpcErrorBody;

// The two error-code maps, split out for the ~200-LoC ceiling when
// `TransactionAborted` (GHSA-r67q-4qqq-g9gm) added an arm to each.
mod codes;

pub use codes::{cratestack_error_code_to_rpc_code, rpc_code};
