//! Top-level client codegen — picks REST or RPC client based on the
//! schema's `transport` directive. Both modes emit the same outer
//! shape (`cratestack_schema::client::Client`, per-model accessors, a
//! `procedures()` sub-client) so downstream call sites don't have to
//! know which path was taken; the methods on the inner clients differ.

mod rest;
mod rpc;

use cratestack_core::{Model, Procedure, TransportStyle};

pub(crate) fn generate_client_module(
    models: &[Model],
    procedures: &[Procedure],
    transport: TransportStyle,
) -> Result<proc_macro2::TokenStream, String> {
    match transport {
        TransportStyle::Rest => rest::generate_generated_client_module(models, procedures),
        TransportStyle::Rpc => rpc::generate_generated_rpc_client_module(models, procedures),
        // A `transport grpc` schema never builds a `cratestack_schema::
        // client::Client` (this fn's own `client` module shape) — its
        // generated client lives at `cratestack_schema::grpc::Client`
        // instead (`include::client::grpc`, ticket #209), parallel to how
        // `include::server::grpc` mounts its tonic service at
        // `cratestack_schema::grpc::into_router` rather than reusing the
        // REST/RPC `axum_module`. This call site is shared by both
        // `include_server_schema!` (building the server's own embedded
        // self/peer-calling client) and `include_client_schema!`
        // (building the consumer-facing client) — both `guard_server_
        // grpc_transport` (#171) and `guard_client_grpc_transport` (#209)
        // let a `Grpc` schema reach this fn behind the `grpc` feature, so
        // erroring here would make every `transport grpc` schema
        // uncompilable under either macro. Emitting nothing is correct
        // for both: `cratestack_schema::client` simply doesn't exist for
        // a `transport grpc` schema, full stop — the real client (or
        // service) is built by the schema-transport-aware `grpc` module
        // each composer splices in separately.
        TransportStyle::Grpc => Ok(quote::quote! {}),
    }
}
