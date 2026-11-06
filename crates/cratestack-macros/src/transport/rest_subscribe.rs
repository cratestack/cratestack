//! REST's subscription route for an `@@subscribe` model,
//! `GET /<plural>/$subscribe` (`docs/design/subscriptions-websocket.md` §7.2,
//! slice S3): its route-transport descriptor, and the `ROUTE_TRANSPORTS`
//! entry that keeps it in the client contract (`op_contract_parity`).

use cratestack_core::{Model, ModelVerb};
use quote::quote;

use super::model_read_transport_capabilities_tokens;
use super::rest::route_transport_const_ident;

/// The descriptor const and its `ROUTE_TRANSPORTS` entry; `None` for a model
/// without `@@subscribe`.
pub(crate) fn generate_model_subscribe_route_transport(
    model: &Model,
) -> Option<(proc_macro2::TokenStream, proc_macro2::TokenStream)> {
    if !cratestack_core::model_verbs(model).contains(&ModelVerb::Subscribe) {
        return None;
    }
    let model_name = model.name.as_str();
    let const_ident = route_transport_const_ident("model", model_name, "subscribe_get");
    let path = cratestack_core::model_subscribe_route(model_name);
    let caps = model_read_transport_capabilities_tokens();
    let constant = quote! {
        pub const #const_ident: ::cratestack::RouteTransportDescriptor = ::cratestack::RouteTransportDescriptor {
            name: #model_name,
            method: "GET",
            path: #path,
            capabilities: #caps,
            // A stream: reconnecting is a fresh subscription, never a replay.
            idempotent_by_default: true,
            rate_limited_by_default: true,
        };
    };
    Some((constant, quote! { #const_ident }))
}
