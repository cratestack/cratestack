//! Transport-binding token generation.
//!
//! Three independent slices live as sibling submodules: REST per-route
//! descriptors ([`rest`]), RPC op descriptors ([`op_descriptors`]),
//! and RPC dispatch arms ([`rpc`]). The top-level macro picks which
//! slice is populated at emission time based on `Schema.transport`.

mod op_descriptors;
mod rest;
mod rpc;

pub(crate) use op_descriptors::{generate_model_op_descriptors, generate_procedure_op_descriptor};
pub(crate) use rest::{
    generate_model_transport_constants, generate_model_transport_entries,
    generate_procedure_transport_constants, generate_procedure_transport_entries,
    model_read_transport_capabilities_tokens, model_write_transport_capabilities_tokens,
    procedure_transport_capabilities_tokens, route_transport_const_ident,
};
pub(crate) use rpc::{generate_model_rpc_dispatch_arms, generate_procedure_rpc_dispatch_arm};
