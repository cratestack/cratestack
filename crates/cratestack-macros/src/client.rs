//! Top-level client codegen — picks REST or RPC client based on the
//! schema's `transport` directive. Both modes emit the same outer
//! shape (`cratestack_schema::client::Client`, per-model accessors, a
//! `procedures()` sub-client) so downstream call sites don't have to
//! know which path was taken; the methods on the inner clients differ.
//!
//! This single call site is shared by both `include_server_schema!`
//! (building the server's own embedded self/peer-calling client) and
//! `include_client_schema!` (building the consumer-facing client) — the
//! composers differ only in what else they splice in around this module,
//! not in how the client itself is generated. `TransportStyle` used to
//! have a third variant, `Grpc`, whose codegen lived entirely outside this
//! function (a hand-rolled tonic service/client under `include::server::
//! grpc` / `include::client::grpc`); this `match` had a third arm that
//! emitted nothing for it, since a gRPC schema never built a
//! `cratestack_schema::client::Client` at all. gRPC support (and that
//! third arm) was removed in v0.9 — `TransportStyle` is Rest/Rpc only now,
//! so this function's two arms are exhaustive without a wildcard.

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
    }
}
