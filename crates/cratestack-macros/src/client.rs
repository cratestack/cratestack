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
//! third arm) was removed in 0.8.5 — `TransportStyle` is Rest/Rpc only now,
//! so this function's two arms are exhaustive without a wildcard.

mod rest;
mod rpc;

use std::collections::BTreeSet;

use cratestack_core::{Model, Procedure, TransportStyle};
use quote::quote;

use crate::shared::ident;

/// A model's response decode target: `super::wire::<Model>` when it's
/// computed-bearing, else the plain `super::models::<Model>` — shared by
/// the REST and RPC per-model client generators (`client::rest::model`,
/// `client::rpc::model`) so the substitution rule can't drift between
/// transports. See [`generate_client_module`]'s doc for what `bearing`
/// means.
pub(super) fn model_output_type_tokens(
    model_name: &str,
    bearing: &BTreeSet<String>,
) -> proc_macro2::TokenStream {
    let model_ident = ident(model_name);
    if bearing.contains(model_name) {
        quote! { super::wire::#model_ident }
    } else {
        quote! { super::models::#model_ident }
    }
}

/// `bearing` — the schema-wide computed-bearing set
/// (`crate::computed::computed_bearing_names`) — decides which decode
/// targets point at the server's dedicated `wire` module
/// (`crate::computed::wire`) instead of the plain server-side
/// `models`/`types` shape (`docs/design/computed-fields.md`'s
/// "Exclusions" section). `include_server_schema!` passes the schema's
/// real bearing set; `include_client_schema!` always passes an empty
/// one — its own `models`/`types` module IS the wire shape already (built
/// by `generate_client_model_struct`/`generate_client_type_struct`), so
/// there is no separate `wire` module to redirect to and this must never
/// fire there (verified by `include::client`'s zero-blast-radius pin).
pub(crate) fn generate_client_module(
    models: &[Model],
    procedures: &[Procedure],
    transport: TransportStyle,
    bearing: &BTreeSet<String>,
) -> Result<proc_macro2::TokenStream, String> {
    match transport {
        TransportStyle::Rest => rest::generate_generated_client_module(models, procedures, bearing),
        TransportStyle::Rpc => {
            rpc::generate_generated_rpc_client_module(models, procedures, bearing)
        }
    }
}
