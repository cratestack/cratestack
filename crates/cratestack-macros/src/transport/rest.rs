//! REST binding: per-procedure / per-model `RouteTransportDescriptor`
//! consts and entries used by the generated router.

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

    Ok(quote! {
        pub const #const_ident: ::cratestack::RouteTransportDescriptor = ::cratestack::RouteTransportDescriptor {
            name: #name,
            method: "POST",
            path: #path,
            capabilities: #capabilities,
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

    quote! {
        pub const #list_ident: ::cratestack::RouteTransportDescriptor = ::cratestack::RouteTransportDescriptor {
            name: #model_name,
            method: "GET",
            path: #list_path,
            capabilities: #read_caps,
        };
        pub const #create_ident: ::cratestack::RouteTransportDescriptor = ::cratestack::RouteTransportDescriptor {
            name: #model_name,
            method: "POST",
            path: #list_path,
            capabilities: #write_caps,
        };
        pub const #get_ident: ::cratestack::RouteTransportDescriptor = ::cratestack::RouteTransportDescriptor {
            name: #model_name,
            method: "GET",
            path: #detail_path,
            capabilities: #read_caps,
        };
        pub const #update_ident: ::cratestack::RouteTransportDescriptor = ::cratestack::RouteTransportDescriptor {
            name: #model_name,
            method: "PATCH",
            path: #detail_path,
            capabilities: #write_caps,
        };
        pub const #delete_ident: ::cratestack::RouteTransportDescriptor = ::cratestack::RouteTransportDescriptor {
            name: #model_name,
            method: "DELETE",
            path: #detail_path,
            capabilities: #read_caps,
        };
    }
}

pub(crate) fn generate_model_transport_entries(model: &Model) -> Vec<proc_macro2::TokenStream> {
    let model_name = &model.name;
    [
        "list_get",
        "list_post",
        "detail_get",
        "detail_patch",
        "detail_delete",
    ]
    .into_iter()
    .map(|suffix| {
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
