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
        // Unreachable in practice: `include::parse::parse_schema_literal`'s
        // `reject_grpc_transport_without_runtime` guard rejects every
        // `transport grpc` schema, for all three entry macros, before any
        // codegen (including this fn) is ever reached. This arm exists
        // only so the match stays exhaustive against `TransportStyle`
        // ticket #170 added a third variant to — see
        // `docs/design/protobuf.md` §9 ticket 5 for the real gRPC client.
        TransportStyle::Grpc => Err("transport grpc has no Rust client codegen yet (tracking: \
             https://github.com/cratestack/cratestack/issues/172); this should be \
             unreachable because `parse_schema_literal` rejects the schema earlier"
            .to_owned()),
    }
}
