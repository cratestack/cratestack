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
        // `include_client_schema!` against a `transport grpc` schema is
        // rejected up front by
        // `reject_grpc::guard_client_or_embedded_grpc_transport` — no Rust
        // gRPC client codegen exists (tracking:
        // https://github.com/cratestack/cratestack/issues/172). This call
        // site is different: `include_server_schema!` calls
        // `generate_client_module` unconditionally too, to build the
        // server's own embedded self/peer-calling client
        // (`cratestack_schema::client::Client`) — and ticket #171's
        // `guard_server_grpc_transport` *does* let a `Grpc` schema reach
        // this fn (behind the `grpc` feature). Erroring here would make
        // every `transport grpc` server schema uncompilable, so this arm
        // emits nothing instead: `cratestack_schema::client` simply
        // doesn't exist for a `transport grpc` schema today. A Rust gRPC
        // client (tonic-based) is future work, not this ticket's scope.
        TransportStyle::Grpc => Ok(quote::quote! {}),
    }
}
