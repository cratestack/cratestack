//! ETag header handling for the get + update + delete handlers,
//! derived from the `@version` field if the model has one. Empty
//! tokens otherwise.
//!
//! `delete_if_match_decl` deliberately mirrors `update_if_match_decl`
//! token-for-token (cratestack#519): `DELETE` on an `@version` model
//! now enforces `If-Match` exactly like `PATCH` does, closing the
//! asymmetry where a stale or absent `If-Match` was silently ignored
//! on delete.

use quote::quote;

use crate::shared::ident;

pub(super) struct EtagTokens {
    pub(super) update_if_match_decl: proc_macro2::TokenStream,
    pub(super) update_if_match_apply: proc_macro2::TokenStream,
    pub(super) update_etag_extract: proc_macro2::TokenStream,
    pub(super) update_etag_apply: proc_macro2::TokenStream,
    pub(super) delete_if_match_decl: proc_macro2::TokenStream,
    pub(super) delete_if_match_apply: proc_macro2::TokenStream,
    pub(super) get_etag_extract_decl: proc_macro2::TokenStream,
    pub(super) get_etag_capture: proc_macro2::TokenStream,
    pub(super) get_etag_apply: proc_macro2::TokenStream,
}

pub(super) fn etag_tokens(
    version_field_name: &Option<String>,
    model_ident: &syn::Ident,
) -> EtagTokens {
    let Some(name) = version_field_name else {
        return EtagTokens {
            update_if_match_decl: quote! {},
            update_if_match_apply: quote! {},
            update_etag_extract: quote! {},
            update_etag_apply: quote! {},
            delete_if_match_decl: quote! {},
            delete_if_match_apply: quote! {},
            get_etag_extract_decl: quote! {},
            get_etag_capture: quote! {},
            get_etag_apply: quote! {},
        };
    };
    let version_field_ident = ident(name);
    EtagTokens {
        update_if_match_decl: quote! {
            let if_match_version = match ::cratestack::parse_if_match_version(&headers) {
                Ok(Some(v)) => Some(v),
                Ok(None) => {
                    return ::cratestack::encode_transport_result_with_status_for::<_, super::models::#model_ident>(
                        &state.codec,
                        &headers,
                        &CAPABILITIES,
                        axum::http::StatusCode::OK,
                        Err(CoolError::PreconditionFailed("If-Match header required".to_owned())),
                    );
                }
                Err(error) => {
                    return ::cratestack::encode_transport_result_with_status_for::<_, super::models::#model_ident>(
                        &state.codec,
                        &headers,
                        &CAPABILITIES,
                        axum::http::StatusCode::OK,
                        Err(error),
                    );
                }
            };
        },
        update_if_match_apply: quote! { .if_match(if_match_version.unwrap()) },
        update_etag_extract: quote! {
            let etag_version: Option<i64> = match &result {
                Ok(record) => Some(record.#version_field_ident),
                Err(_) => None,
            };
        },
        update_etag_apply: quote! {
            if let Some(v) = etag_version {
                ::cratestack::set_version_etag(&mut response, v);
            }
        },
        delete_if_match_decl: quote! {
            let if_match_version = match ::cratestack::parse_if_match_version(&headers) {
                Ok(Some(v)) => Some(v),
                Ok(None) => {
                    return ::cratestack::encode_transport_result_with_status_for::<_, super::models::#model_ident>(
                        &state.codec,
                        &headers,
                        &CAPABILITIES,
                        axum::http::StatusCode::OK,
                        Err(CoolError::PreconditionFailed("If-Match header required".to_owned())),
                    );
                }
                Err(error) => {
                    return ::cratestack::encode_transport_result_with_status_for::<_, super::models::#model_ident>(
                        &state.codec,
                        &headers,
                        &CAPABILITIES,
                        axum::http::StatusCode::OK,
                        Err(error),
                    );
                }
            };
        },
        delete_if_match_apply: quote! { .if_match(if_match_version.unwrap()) },
        get_etag_extract_decl: quote! {
            let mut etag_version: Option<i64> = None;
        },
        get_etag_capture: quote! {
            etag_version = Some(record.#version_field_ident);
        },
        get_etag_apply: quote! {
            if let Some(v) = etag_version {
                ::cratestack::set_version_etag(&mut response, v);
            }
        },
    }
}
