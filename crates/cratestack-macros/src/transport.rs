//! Transport-binding token generation.
//!
//! Four independent slices live as sibling submodules: REST per-route
//! descriptors ([`rest`]), RPC op descriptors ([`op_descriptors`]), RPC
//! unary/batch dispatch arms ([`rpc`]), and RPC subscription (SSE)
//! dispatch arms ([`subscribe_dispatch`]). The top-level macro picks
//! which slice is populated at emission time based on `Schema.transport`.

mod op_descriptors;
mod rest;
mod rpc;
mod subscribe_dispatch;

pub(crate) use op_descriptors::{
    generate_model_op_descriptors, generate_model_subscribe_op_descriptor,
    generate_procedure_op_descriptor,
};
pub(crate) use rest::{
    generate_model_transport_constants, generate_model_transport_entries,
    generate_procedure_transport_constants, generate_procedure_transport_entries,
    model_read_transport_capabilities_tokens, model_write_transport_capabilities_tokens,
    procedure_transport_capabilities_tokens,
};
pub(crate) use rpc::{generate_model_rpc_dispatch_arms, generate_procedure_rpc_dispatch_arm};
pub(crate) use subscribe_dispatch::generate_model_subscribe_dispatch_arm;
