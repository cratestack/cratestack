//! `list`/`get` method builders for the REST per-model client, split out
//! of `client/rest/model.rs` per the repo's 200-LoC file convention.
//! Both methods are gated on `computed_params_ident`
//! (`crate::client::computed_params::model_computed_params_ident`) — an
//! ungated model (`None`) keeps the exact tokens this function emitted
//! before the typed `computedParams` surface existed
//! (`docs/design/computed-fields.md`'s "Downstream" section).

use proc_macro2::TokenStream;
use quote::quote;

/// `pub async fn list(...)` — with a `computed_params` parameter folded
/// into a synthesized `?computedParams=` query pair when
/// `computed_params_ident` is `Some`, or the original ungated signature
/// otherwise.
pub(super) fn build_list_method(
    computed_params_ident: Option<&syn::Ident>,
    route_path: &str,
    list_output_type: &TokenStream,
) -> TokenStream {
    match computed_params_ident {
        Some(computed_params_ident) => quote! {
            pub async fn list(
                &self,
                query: &[::cratestack::client_rust::QueryPair<'_>],
                computed_params: ::core::option::Option<&#computed_params_ident>,
                headers: &[::cratestack::client_rust::HeaderPair<'_>],
            ) -> Result<#list_output_type, ::cratestack::client_rust::ClientError> {
                let computed_params_value =
                    computed_params.and_then(|params| params.to_query_value());
                let mut full_query: ::std::vec::Vec<::cratestack::client_rust::QueryPair<'_>> =
                    query.to_vec();
                if let Some(value) = &computed_params_value {
                    full_query.push(("computedParams", value.as_str()));
                }
                self.runtime.get(#route_path, &full_query, headers).await
            }
        },
        None => quote! {
            pub async fn list(
                &self,
                query: &[::cratestack::client_rust::QueryPair<'_>],
                headers: &[::cratestack::client_rust::HeaderPair<'_>],
            ) -> Result<#list_output_type, ::cratestack::client_rust::ClientError> {
                self.runtime.get(#route_path, query, headers).await
            }
        },
    }
}

/// `pub async fn get(...)` — with a `computed_params` parameter folded
/// into a synthesized `?computedParams=` query pair when
/// `computed_params_ident` is `Some`, or the original ungated signature
/// otherwise.
pub(super) fn build_get_method(
    computed_params_ident: Option<&syn::Ident>,
    route_path: &str,
    primary_key_type: &TokenStream,
    model_output_type: &TokenStream,
) -> TokenStream {
    match computed_params_ident {
        Some(computed_params_ident) => quote! {
            pub async fn get(
                &self,
                id: &#primary_key_type,
                computed_params: ::core::option::Option<&#computed_params_ident>,
                headers: &[::cratestack::client_rust::HeaderPair<'_>],
            ) -> Result<#model_output_type, ::cratestack::client_rust::ClientError> {
                let computed_params_value =
                    computed_params.and_then(|params| params.to_query_value());
                let query: &[::cratestack::client_rust::QueryPair<'_>] =
                    match &computed_params_value {
                        Some(value) => &[("computedParams", value.as_str())],
                        None => &[],
                    };
                self.runtime.get(&format!("{}/{}", #route_path, id), query, headers).await
            }
        },
        None => quote! {
            pub async fn get(
                &self,
                id: &#primary_key_type,
                headers: &[::cratestack::client_rust::HeaderPair<'_>],
            ) -> Result<#model_output_type, ::cratestack::client_rust::ClientError> {
                self.runtime.get(&format!("{}/{}", #route_path, id), &[], headers).await
            }
        },
    }
}
