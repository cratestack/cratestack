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

mod computed;
mod with_response;
use computed::{build_get_method, build_list_method};
use with_response::{
    build_delete_with_response_method, build_get_with_response_method,
    build_update_with_response_method,
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
    // Builders live in `model/computed.rs` (200-LoC file convention).
    let list_method = build_list_method(computed_params_ident, &route_path, &list_output_type);
    let get_method = build_get_method(
        computed_params_ident,
        &route_path,
        &primary_key_type,
        &model_output_type,
    );
    let get_with_response_method =
        build_get_with_response_method(&route_path, &primary_key_type, &model_output_type);
    let update_with_response_method = build_update_with_response_method(
        &route_path,
        &primary_key_type,
        &update_input_ident,
        &model_output_type,
    );
    let delete_with_response_method =
        build_delete_with_response_method(&route_path, &primary_key_type, &model_output_type);

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

            #get_with_response_method

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

            #update_with_response_method

            pub async fn delete(
                &self,
                id: &#primary_key_type,
                headers: &[::cratestack::client_rust::HeaderPair<'_>],
            ) -> Result<#model_output_type, ::cratestack::client_rust::ClientError> {
                self.runtime.delete(&format!("{}/{}", #route_path, id), headers).await
            }

            #delete_with_response_method
        }
    })
}
