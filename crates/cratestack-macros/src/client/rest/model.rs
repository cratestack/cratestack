//! REST per-model client struct: `<Model>Client` with list / get /
//! create / update / delete (plus `*_view` projection variants on
//! list/get). Paged models return `Page<Model>`; non-paged return
//! `Vec<Model>`.

use std::collections::BTreeSet;

use cratestack_core::Model;
use quote::quote;

use crate::client::model_output_type_tokens;
use crate::shared::{
    ident, is_paged_model, is_primary_key, pluralize, rust_type_tokens, to_snake_case,
};

pub(super) fn generate_generated_model_client(
    model: &Model,
    bearing: &BTreeSet<String>,
    computed_params_ident: Option<&syn::Ident>,
) -> Result<proc_macro2::TokenStream, String> {
    let client_ident = ident(&format!("{}Client", model.name));
    let create_input_ident = ident(&format!("Create{}Input", model.name));
    let update_input_ident = ident(&format!("Update{}Input", model.name));
    let route_path = format!("/{}", pluralize(&to_snake_case(&model.name)));
    let paged = is_paged_model(model);
    let primary_key = model
        .fields
        .iter()
        .find(|field| is_primary_key(field))
        .ok_or_else(|| format!("model {} is missing a primary key", model.name))?;
    let primary_key_type = rust_type_tokens(&primary_key.ty);
    let model_output_type = model_output_type_tokens(&model.name, bearing);
    let list_output_type = if paged {
        quote! { ::cratestack::Page<#model_output_type> }
    } else {
        quote! { Vec<#model_output_type> }
    };
    let list_view_output_type = if paged {
        quote! { ::cratestack::Page<P::Output> }
    } else {
        quote! { Vec<P::Output> }
    };
    let list_view_call = if paged {
        quote! {
            self.runtime
                .list_view_paged(#route_path, projection, query, headers)
                .await
        }
    } else {
        quote! {
            self.runtime
                .list_view(#route_path, projection, query, headers)
                .await
        }
    };

    // `computed_params_ident` gates both `list` and `get` on whether this
    // model declares at least one parameterized `@computed` field
    // (`crate::client::computed_params::model_computed_params_ident`) —
    // an ungated model keeps the exact tokens this function emitted
    // before this feature existed (see that module's doc for why).
    let list_method = match computed_params_ident {
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
    };
    let get_method = match computed_params_ident {
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
    };

    Ok(quote! {
        #[derive(Clone)]
        pub struct #client_ident<C = ::cratestack::client_rust::CborCodec>
        where
            C: ::cratestack::client_rust::HttpClientCodec,
        {
            runtime: ::cratestack::client_rust::CratestackClient<C>,
        }

        impl<C> #client_ident<C>
        where
            C: ::cratestack::client_rust::HttpClientCodec,
        {
            fn new(runtime: ::cratestack::client_rust::CratestackClient<C>) -> Self {
                Self { runtime }
            }

            #list_method

            pub async fn list_view<P>(
                &self,
                projection: &P,
                query: &[::cratestack::client_rust::QueryPair<'_>],
                headers: &[::cratestack::client_rust::HeaderPair<'_>],
            ) -> Result<#list_view_output_type, ::cratestack::client_rust::ClientError>
            where
                P: ::cratestack::ProjectionDecoder,
            {
                #list_view_call
            }

            #get_method

            /// Same call as [`Self::get`], but returns the status and
            /// response headers alongside the record (issue #493) — read
            /// `TypedResponse::header("etag")` off the result to get the
            /// value [`Self::update_with_response`] needs as `If-Match`
            /// on an `@version` model. `delete_with_response` needs it
            /// too, since cratestack#519: the server enforces `If-Match`
            /// on `DELETE` exactly like `PATCH` (see
            /// [`Self::delete_with_response`]).
            pub async fn get_with_response(
                &self,
                id: &#primary_key_type,
                headers: &[::cratestack::client_rust::HeaderPair<'_>],
            ) -> Result<
                ::cratestack::client_rust::TypedResponse<#model_output_type>,
                ::cratestack::client_rust::ClientError,
            > {
                self.runtime.get_with_response(&format!("{}/{}", #route_path, id), &[], headers).await
            }

            pub async fn get_view<P>(
                &self,
                id: &#primary_key_type,
                projection: &P,
                headers: &[::cratestack::client_rust::HeaderPair<'_>],
            ) -> Result<P::Output, ::cratestack::client_rust::ClientError>
            where
                P: ::cratestack::ProjectionDecoder,
            {
                self.runtime
                    .get_view(&format!("{}/{}", #route_path, id), projection, headers)
                    .await
            }

            pub async fn create(
                &self,
                input: &super::inputs::#create_input_ident,
                headers: &[::cratestack::client_rust::HeaderPair<'_>],
            ) -> Result<#model_output_type, ::cratestack::client_rust::ClientError> {
                self.runtime.post(#route_path, input, headers).await
            }

            pub async fn update(
                &self,
                id: &#primary_key_type,
                input: &super::inputs::#update_input_ident,
                headers: &[::cratestack::client_rust::HeaderPair<'_>],
            ) -> Result<#model_output_type, ::cratestack::client_rust::ClientError> {
                self.runtime.patch(&format!("{}/{}", #route_path, id), input, headers).await
            }

            /// Same call as [`Self::update`], but returns the status and
            /// response headers alongside the record (issue #493) — on
            /// an `@version` model, `headers` must carry `If-Match`
            /// (from a prior [`Self::get_with_response`]), and the
            /// response's `ETag` is the value a chained update needs
            /// next.
            pub async fn update_with_response(
                &self,
                id: &#primary_key_type,
                input: &super::inputs::#update_input_ident,
                headers: &[::cratestack::client_rust::HeaderPair<'_>],
            ) -> Result<
                ::cratestack::client_rust::TypedResponse<#model_output_type>,
                ::cratestack::client_rust::ClientError,
            > {
                self.runtime.patch_with_response(&format!("{}/{}", #route_path, id), input, headers).await
            }

            pub async fn delete(
                &self,
                id: &#primary_key_type,
                headers: &[::cratestack::client_rust::HeaderPair<'_>],
            ) -> Result<#model_output_type, ::cratestack::client_rust::ClientError> {
                self.runtime.delete(&format!("{}/{}", #route_path, id), headers).await
            }

            /// Same call as [`Self::delete`], but returns the status and
            /// response headers alongside the record (issue #493) — for
            /// reading e.g. a `Retry-After` on a `429`, or any other
            /// out-of-band signal a server sends on a delete response.
            ///
            /// Part of the `@version` optimistic-locking round trip
            /// since cratestack#519: like [`Self::update_with_response`],
            /// the server requires `If-Match` in `headers` on an
            /// `@version` model and returns `412` on a stale or missing
            /// value.
            pub async fn delete_with_response(
                &self,
                id: &#primary_key_type,
                headers: &[::cratestack::client_rust::HeaderPair<'_>],
            ) -> Result<
                ::cratestack::client_rust::TypedResponse<#model_output_type>,
                ::cratestack::client_rust::ClientError,
            > {
                self.runtime.delete_with_response(&format!("{}/{}", #route_path, id), headers).await
            }
        }
    })
}
