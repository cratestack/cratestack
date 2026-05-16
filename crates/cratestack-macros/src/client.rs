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
    }
}
