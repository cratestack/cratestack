//! Transport-binding token generation.
//!
//! Four independent slices live as sibling submodules: REST per-route
//! descriptors ([`rest`]), RPC op descriptors ([`op_descriptors`]), RPC
//! unary/batch dispatch arms ([`rpc`]), subscription descriptors
//! ([`rest_subscribe`], [`subscription_procedure_descriptors`]), and the
//! transport-neutral subscription dispatcher every binding opens through
//! ([`subscription_module`], with [`topics_module`]). The top-level macro picks
//! which slice is populated at emission time based on `Schema.transport`.

mod idempotency;
mod op_descriptors;
mod rate_limit;
mod rest;
mod rest_subscribe;
mod rpc;
mod subscription_module;
mod subscription_procedure_descriptors;
mod topics_module;

pub(crate) use op_descriptors::{
    generate_model_op_descriptors, generate_model_subscribe_op_descriptor,
    generate_procedure_op_descriptor, model_get_op_descriptor, model_list_op_descriptor,
};
pub(crate) use rest::{
    generate_model_transport_constants, generate_model_transport_entries,
    generate_procedure_transport_constants, generate_procedure_transport_entries,
    model_read_transport_capabilities_tokens, model_write_transport_capabilities_tokens,
    procedure_transport_capabilities_tokens,
};
pub(crate) use rest_subscribe::generate_model_subscribe_route_transport;
pub(crate) use rpc::{generate_model_rpc_dispatch_arms, generate_procedure_rpc_dispatch_arm};
pub(crate) use subscription_module::{SubscriptionSchema, generate_subscription_module};
pub(crate) use subscription_procedure_descriptors::{
    generate_subscription_procedure_op_descriptor, generate_subscription_procedure_route_transport,
};
pub(crate) use topics_module::generate_topics_module;
