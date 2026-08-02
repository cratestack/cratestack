//! Route path + header token helpers driven by procedure attributes:
//! `@api_version("...")` (route prefix) and `@deprecated`/`@deprecated("...")`
//! (`Deprecation`/`X-Deprecation` response headers). Split out of
//! `super` to keep that file under the workspace's ~200-LoC-per-file
//! convention.

use cratestack_core::Procedure;
use quote::quote;

use crate::shared::{ident, to_snake_case};

pub(super) fn procedure_axum_route_tokens(procedure: &Procedure) -> proc_macro2::TokenStream {
    let route_path = procedure_route_path(procedure);
    let handler_ident = ident(&format!("handle_{}", to_snake_case(&procedure.name)));
    quote! { .route(#route_path, axum::routing::post(#handler_ident)) }
}

/// HTTP route path for a procedure, applying any `@api_version`
/// prefix. Shape is `/<version>/$procs/<name>` for versioned
/// procedures and `/$procs/<name>` otherwise, so banks can run v1 + v2
/// side by side.
pub(super) fn procedure_route_path(procedure: &Procedure) -> String {
    if let Some(version) = procedure_api_version(procedure) {
        format!("/{}/$procs/{}", version, procedure.name)
    } else {
        format!("/$procs/{}", procedure.name)
    }
}

fn procedure_api_version(procedure: &Procedure) -> Option<String> {
    procedure.attributes.iter().find_map(|attribute| {
        attribute
            .raw
            .strip_prefix("@api_version(\"")
            .and_then(|rest| rest.strip_suffix("\")"))
            .map(|s| s.to_owned())
    })
}

/// Token stream that, given a `response` in scope, applies the
/// `Deprecation`/`X-Deprecation` headers when the procedure declared
/// `@deprecated`. Empty tokens for non-deprecated procedures.
pub(super) fn procedure_deprecation_header_tokens(
    procedure: &Procedure,
) -> proc_macro2::TokenStream {
    let deprecated = procedure
        .attributes
        .iter()
        .find(|a| a.raw == "@deprecated" || a.raw.starts_with("@deprecated("));
    let Some(attribute) = deprecated else {
        return quote! {};
    };
    let message: Option<String> = attribute
        .raw
        .strip_prefix("@deprecated(\"")
        .and_then(|s| s.strip_suffix("\")"))
        .map(|s| s.to_owned());
    let message_block = match message {
        Some(m) => quote! {
            if let Ok(value) = ::cratestack::axum::http::HeaderValue::from_str(#m) {
                response.headers_mut().insert("X-Deprecation", value);
            }
        },
        None => quote! {},
    };
    quote! {
        response
            .headers_mut()
            .insert("Deprecation", ::cratestack::axum::http::HeaderValue::from_static("true"));
        #message_block
    }
}
