//! REST binding: per-procedure / per-model `RouteTransportDescriptor`
//! consts and entries used by the generated router.

#[cfg(test)]
mod tests;

use cratestack_core::{Model, Procedure, TypeArity};
use quote::quote;

use crate::shared::{ident, pluralize, to_snake_case};

pub(crate) fn generate_procedure_transport_constants(
    procedure: &Procedure,
) -> Result<proc_macro2::TokenStream, String> {
    let const_ident = route_transport_const_ident("procedure", &procedure.name, "post");
    let path = format!("/$procs/{}", procedure.name);
    let capabilities = procedure_transport_capabilities_tokens(procedure);
    let name = procedure.name.as_str();
    let rate_limited = super::rate_limit::procedure_rate_limited_by_default(procedure);

    Ok(quote! {
        pub const #const_ident: ::cratestack::RouteTransportDescriptor = ::cratestack::RouteTransportDescriptor {
            name: #name,
            method: "POST",
            path: #path,
            capabilities: #capabilities,
            rate_limited_by_default: #rate_limited,
        };
    })
}

pub(crate) fn generate_procedure_transport_entries(
    procedure: &Procedure,
) -> proc_macro2::TokenStream {
    let const_ident = route_transport_const_ident("procedure", &procedure.name, "post");
    quote! { #const_ident }
}

pub(crate) fn generate_model_transport_constants(model: &Model) -> proc_macro2::TokenStream {
    let model_name = &model.name;
    let list_path = format!("/{}", pluralize(&to_snake_case(model_name)));
    let detail_path = format!("/{}/{{id}}", pluralize(&to_snake_case(model_name)));

    let list_ident = route_transport_const_ident("model", model_name, "list_get");
    let create_ident = route_transport_const_ident("model", model_name, "list_post");
    let get_ident = route_transport_const_ident("model", model_name, "detail_get");
    let update_ident = route_transport_const_ident("model", model_name, "detail_patch");
    let delete_ident = route_transport_const_ident("model", model_name, "detail_delete");

    let read_caps = model_read_transport_capabilities_tokens();
    let write_caps = model_write_transport_capabilities_tokens();
    // Model CRUD routes have no `@no_rate_limit`-equivalent opt-out today
    // (that attribute is procedure-only, per docs/design/extensions.md
    // §5), so every one of them always participates in rate limiting.
    // Mirrors `generate_model_op_descriptors`'s identical `rate_limited`.
    let rate_limited = true;

    // `cratestack_core::model_internal_actions` — the same single
    // source of truth `axum/model/routes.rs`,
    // `transport/op_descriptors.rs`, `transport/rpc/model_dispatch.rs`
    // and `client/rest/model.rs` already consult (cratestack#743,
    // `docs/design/route-suppression.md` §1.1, "a second place any fix
    // has to touch"). `ROUTE_TRANSPORTS` is a `pub const` in the
    // generated crate's public API — listing a verb the schema author
    // explicitly suppressed is wrong on its face, independent of the
    // fact that the only runtime reader today
    // (`cratestack-axum/src/ratelimit/rest_ops_filter.rs`) already
    // fails closed (rate-limits) on a lookup miss, so omitting the
    // entry changes nothing about that filter's behavior.
    let internal = cratestack_core::model_internal_actions(model);

    let list_const = (!internal.contains("list")).then(|| {
        quote! {
            pub const #list_ident: ::cratestack::RouteTransportDescriptor = ::cratestack::RouteTransportDescriptor {
                name: #model_name,
                method: "GET",
                path: #list_path,
                capabilities: #read_caps,
                rate_limited_by_default: #rate_limited,
            };
        }
    });
    let create_const = (!internal.contains("create")).then(|| {
        quote! {
            pub const #create_ident: ::cratestack::RouteTransportDescriptor = ::cratestack::RouteTransportDescriptor {
                name: #model_name,
                method: "POST",
                path: #list_path,
                capabilities: #write_caps,
                rate_limited_by_default: #rate_limited,
            };
        }
    });
    let get_const = (!internal.contains("get")).then(|| {
        quote! {
            pub const #get_ident: ::cratestack::RouteTransportDescriptor = ::cratestack::RouteTransportDescriptor {
                name: #model_name,
                method: "GET",
                path: #detail_path,
                capabilities: #read_caps,
                rate_limited_by_default: #rate_limited,
            };
        }
    });
    let update_const = (!internal.contains("update")).then(|| {
        quote! {
            pub const #update_ident: ::cratestack::RouteTransportDescriptor = ::cratestack::RouteTransportDescriptor {
                name: #model_name,
                method: "PATCH",
                path: #detail_path,
                capabilities: #write_caps,
                rate_limited_by_default: #rate_limited,
            };
        }
    });
    let delete_const = (!internal.contains("delete")).then(|| {
        quote! {
            pub const #delete_ident: ::cratestack::RouteTransportDescriptor = ::cratestack::RouteTransportDescriptor {
                name: #model_name,
                method: "DELETE",
                path: #detail_path,
                capabilities: #read_caps,
                rate_limited_by_default: #rate_limited,
            };
        }
    });

    quote! {
        #list_const
        #create_const
        #get_const
        #update_const
        #delete_const
    }
}

pub(crate) fn generate_model_transport_entries(model: &Model) -> Vec<proc_macro2::TokenStream> {
    let model_name = &model.name;
    // Must stay filtered identically to `generate_model_transport_constants`
    // above — an entry referencing a const that was never emitted for a
    // suppressed verb would be a compile error, not merely a stale
    // listing.
    let internal = cratestack_core::model_internal_actions(model);

    [
        ("list_get", "list"),
        ("list_post", "create"),
        ("detail_get", "get"),
        ("detail_patch", "update"),
        ("detail_delete", "delete"),
    ]
    .into_iter()
    .filter(|(_, verb)| !internal.contains(verb))
    .map(|(suffix, _)| {
        let id = route_transport_const_ident("model", model_name, suffix);
        quote! { #id }
    })
    .collect()
}

pub(crate) fn route_transport_const_ident(kind: &str, name: &str, suffix: &str) -> syn::Ident {
    ident(&format!("{}_{}_{}", kind, to_snake_case(name), suffix).to_ascii_uppercase())
}

pub(crate) fn procedure_transport_capabilities_tokens(
    procedure: &Procedure,
) -> proc_macro2::TokenStream {
    if matches!(procedure.return_type.arity, TypeArity::List) {
        quote! {
            ::cratestack::RouteTransportCapabilities {
                request_types: &["application/cbor", "application/json"],
                response_types: &[
                    "application/cbor",
                    "application/json",
                    ::cratestack::CBOR_SEQUENCE_CONTENT_TYPE,
                ],
                default_response_type: "application/cbor",
                supports_sequence_response: true,
            }
        }
    } else {
        quote! {
            ::cratestack::RouteTransportCapabilities {
                request_types: &["application/cbor", "application/json"],
                response_types: &["application/cbor", "application/json"],
                default_response_type: "application/cbor",
                supports_sequence_response: false,
            }
        }
    }
}

pub(crate) fn model_read_transport_capabilities_tokens() -> proc_macro2::TokenStream {
    quote! {
        ::cratestack::RouteTransportCapabilities {
            request_types: &[],
            response_types: &["application/cbor", "application/json"],
            default_response_type: "application/cbor",
            supports_sequence_response: false,
        }
    }
}

pub(crate) fn model_write_transport_capabilities_tokens() -> proc_macro2::TokenStream {
    quote! {
        ::cratestack::RouteTransportCapabilities {
            request_types: &["application/cbor", "application/json"],
            response_types: &["application/cbor", "application/json"],
            default_response_type: "application/cbor",
            supports_sequence_response: false,
        }
    }
}
