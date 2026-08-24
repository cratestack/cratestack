//! Runtime primitives for the `transport rpc` generation style.
//!
//! See `docs/design/rpc-transport.md` for the full design. This module
//! provides the binding-side surface that schemas with `transport rpc`
//! generate against:
//!
//! - `POST /rpc/{op_id}` — unary calls. Body is the codec-encoded *input*
//!   (no frame wrapper); response body is the codec-encoded *output* on
//!   success, or an [`RpcErrorBody`] on error with HTTP status mapped via
//!   [`CratestackError::status_code`].
//! - `POST /rpc/batch` — sequence of `RpcRequest` frames in, sequence of
//!   `RpcResponseFrame` frames out in the same order. Per-frame errors
//!   don't poison the batch.
//! - `GET /rpc/subscribe/{op_id}` — SSE subscription dispatch for
//!   `@@subscribe`d models (design doc §3.4a, cratestack#390). One
//!   `event: message` per `ModelEvent<T>`, terminated by one
//!   `event: error` on backpressure overflow. See [`sse`] and
//!   [`subscription_bridge`].
//!
//! The full WebSocket frame loop (§3.4) remains speced but unbuilt,
//! gated on a real bidirectional/high-multiplexing need per issue
//! #183's spike decision — see `docs/design/rpc-transport.md` §6.5.
//!
//! The macro emits the dispatch table and the `rpc_router` constructor.
//! This crate provides the shared frame shapes, error mapping, and the
//! `RPC_*_PATH` constants both sides agree on.

mod batch;
mod codec_helpers;
mod error_encode;
mod sse;
mod subscription_bridge;
mod synthesize;
mod util;

#[cfg(test)]
mod tests_error;
#[cfg(test)]
mod tests_frame;
#[cfg(test)]
mod tests_get;
#[cfg(test)]
mod tests_list;
#[cfg(test)]
mod tests_response_rebuffer;

// Re-export the wire shapes from `cratestack-core::rpc`. Both the server
// binding and every generated client agree on those shapes, and lifting
// them into core means the client crates don't need to depend on axum.
// `RpcListInput`/`RpcListPredicate`/`RpcPkInput`/`RpcUpdateInput` joined
// this list via cratestack#490 — previously defined locally in this
// crate's own (now-removed) `inputs` module, which meant
// `include_client_schema!`'s RPC model-CRUD codegen (`::cratestack::rpc::
// RpcListInput`, …) could never resolve for a facade without
// `cratestack-axum` in its graph. See `cratestack-core::rpc`'s doc comment
// on those types for the full story.
pub use cratestack_core::rpc::{
    RPC_BATCH_PATH, RPC_STREAM_ERROR_TAG, RPC_SUBSCRIBE_PATH, RPC_UNARY_PATH, RpcErrorBody,
    RpcGetInput, RpcListInput, RpcListPredicate, RpcPkInput, RpcRequest, RpcResponseFrame,
    RpcUpdateInput, cratestack_error_code_to_rpc_code, rpc_code,
};

pub use batch::response_to_frame;
pub use codec_helpers::{decode_rpc_body, encode_rpc_value};
pub use error_encode::{convert_handler_error_response, encode_rpc_error};
pub use sse::{encode_model_event_sse_response, validate_subscribe_accept_header};
pub use subscription_bridge::{SubscriptionPush, guarded_receiver_stream, subscription_channel};
pub use synthesize::{synthesize_get_query, synthesize_list_query};

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
