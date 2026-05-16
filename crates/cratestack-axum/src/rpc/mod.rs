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

mod batch;
mod codec_helpers;
mod error_encode;
mod inputs;
mod synthesize;
mod util;

#[cfg(test)]
mod tests_error;
#[cfg(test)]
mod tests_frame;
#[cfg(test)]
mod tests_list;

// Re-export the wire shapes from `cratestack-core::rpc`. Both the server
// binding and every generated client agree on those shapes, and lifting
// them into core means the client crates don't need to depend on axum.
pub use cratestack_core::rpc::{
    RPC_BATCH_PATH, RPC_UNARY_PATH, RpcErrorBody, RpcRequest, RpcResponseFrame,
    cool_error_code_to_rpc_code, rpc_code,
};

pub use batch::response_to_frame;
pub use codec_helpers::{decode_rpc_body, encode_rpc_value};
pub use error_encode::{convert_handler_error_response, encode_rpc_error};
pub use inputs::{RpcListInput, RpcListPredicate, RpcPkInput, RpcUpdateInput};
pub use synthesize::synthesize_list_query;

/// Codec/transport capabilities for every RPC binding route. Both unary
/// and batch accept and emit CBOR or JSON, default CBOR; sequence
/// responses (streaming) are not yet supported by this binding.
///
/// Used by `encode_transport_result_with_status_for` to negotiate
/// response content type when the dispatcher synthesizes an error
/// response or wraps a batch result.
pub const RPC_BINDING_CAPABILITIES: cratestack_core::RouteTransportCapabilities =
    cratestack_core::RouteTransportCapabilities {
        request_types: &["application/cbor", "application/json"],
        response_types: &["application/cbor", "application/json"],
        default_response_type: "application/cbor",
        supports_sequence_response: false,
    };
