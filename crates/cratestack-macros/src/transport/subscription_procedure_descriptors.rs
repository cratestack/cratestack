//! A `subscription procedure`'s descriptors (slice S4): its RPC op
//! descriptor (`procedure.<name>`, `OpKind::Subscription`) and its REST route
//! transport (`GET <procedure path>`), the entries that keep it in the client
//! contract (`op_contract_parity`).

use cratestack_core::Procedure;
use quote::quote;

use super::model_read_transport_capabilities_tokens;
use super::op_descriptors::op_descriptor;
use super::rest::route_transport_const_ident;

pub(crate) fn generate_subscription_procedure_op_descriptor(
    procedure: &Procedure,
    auth_required: bool,
) -> proc_macro2::TokenStream {
    let op_id = cratestack_core::subscription_procedure_op_key(procedure, true);
    let input_ty = procedure
        .args
        .first()
        .map(|a| a.ty.name.as_str())
        .unwrap_or("");
    // Reopening a stream is a fresh subscription, never a replay.
    op_descriptor(
        &op_id,
        quote! { ::cratestack::OpKind::Subscription },
        input_ty,
        &procedure.return_type.name,
        true,
        true,
        auth_required,
    )
}

/// The REST descriptor const and its `ROUTE_TRANSPORTS` entry.
pub(crate) fn generate_subscription_procedure_route_transport(
    procedure: &Procedure,
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    let name = procedure.name.as_str();
    let const_ident = route_transport_const_ident("procedure", name, "subscribe_get");
    let path = cratestack_core::procedure_route::procedure_rest_route_path(procedure);
    let caps = model_read_transport_capabilities_tokens();
    let constant = quote! {
        pub const #const_ident: ::cratestack::RouteTransportDescriptor = ::cratestack::RouteTransportDescriptor {
            name: #name,
            method: "GET",
            path: #path,
            capabilities: #caps,
            idempotent_by_default: true,
            rate_limited_by_default: true,
        };
    };
    (constant, quote! { #const_ident })
}
