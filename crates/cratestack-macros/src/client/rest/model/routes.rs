//! The route each generated REST call names (cratestack#1007).
//!
//! A signed client binds the route *template* into its COSE envelope, so
//! every call says which template it is for through `CratestackClient::at`
//! (see `RouteRef`). The templates come from the same derivation the server
//! lists in `ROUTE_TRANSPORTS` (`crate::shared::model_routes`), so the two
//! cannot disagree. The detail routes' one parameter is the primary key as
//! the router decodes it: `id.to_string()`, the value the URL is built from.

use proc_macro2::TokenStream;
use quote::quote;

/// The runtime for a call on the collection route.
pub(super) fn list_runtime(route_path: &str) -> TokenStream {
    quote! {
        self.runtime.at(::cratestack::client_rust::RouteRef::new(#route_path, &[]))
    }
}

/// The runtime for a call on the detail route. The call must be preceded by
/// [`bind_id`], which defines `id_text`.
pub(super) fn detail_runtime(route_path: &str) -> TokenStream {
    let template = cratestack_core::detail_route_of(route_path);
    quote! {
        self.runtime.at(::cratestack::client_rust::RouteRef::new(#template, &[id_text.as_str()]))
    }
}

/// `let id_text = id.to_string();`, the value the detail route binds.
pub(super) fn bind_id() -> TokenStream {
    quote! { let id_text = id.to_string(); }
}

/// The concrete detail URL path, for the `id_text` [`bind_id`] defines.
pub(super) fn detail_url(route_path: &str) -> TokenStream {
    // The path carries the id percent-encoded; the seal binds `id_text` raw,
    // which is what the server decodes the segment back to.
    quote! { &format!("{}/{}", #route_path, ::cratestack::client_rust::encode_path_segment(&id_text)) }
}
