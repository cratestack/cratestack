//! REST per-model client struct: `<Model>Client` with list / get /
//! create / update / delete (plus `*_view` projection variants on
//! list/get). Paged models return `Page<Model>`; non-paged return
//! `Vec<Model>`.

use cratestack_core::Model;
use quote::quote;

use crate::shared::{
    ident, is_paged_model, is_primary_key, pluralize, rust_type_tokens, to_snake_case,
};

pub(super) fn generate_generated_model_client(
    model: &Model,
) -> Result<proc_macro2::TokenStream, String> {
    let client_ident = ident(&format!("{}Client", model.name));
    let model_ident = ident(&model.name);
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
    let list_output_type = if paged {
        quote! { ::cratestack::Page<super::models::#model_ident> }
    } else {
        quote! { Vec<super::models::#model_ident> }
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

            pub async fn list(
                &self,
                query: &[::cratestack::client_rust::QueryPair<'_>],
                headers: &[::cratestack::client_rust::HeaderPair<'_>],
            ) -> Result<#list_output_type, ::cratestack::client_rust::ClientError> {
                self.runtime.get(#route_path, query, headers).await
            }

            pub async fn list_view<P>(
                &self,
                projection: &P,
                query: &[::cratestack::client_rust::QueryPair<'_>],
                headers: &[::cratestack::client_rust::HeaderPair<'_>],
            ) -> Result<#list_view_output_type, ::cratestack::client_rust::ClientError>
            where
                P: ::cratestack::client_rust::Projection,
            {
                #list_view_call
            }

            pub async fn get(
                &self,
                id: &#primary_key_type,
                headers: &[::cratestack::client_rust::HeaderPair<'_>],
            ) -> Result<super::models::#model_ident, ::cratestack::client_rust::ClientError> {
                self.runtime.get(&format!("{}/{}", #route_path, id), &[], headers).await
            }

            pub async fn get_view<P>(
                &self,
                id: &#primary_key_type,
                projection: &P,
                headers: &[::cratestack::client_rust::HeaderPair<'_>],
            ) -> Result<P::Output, ::cratestack::client_rust::ClientError>
            where
                P: ::cratestack::client_rust::Projection,
            {
                self.runtime
                    .get_view(&format!("{}/{}", #route_path, id), projection, headers)
                    .await
            }

            pub async fn create(
                &self,
                input: &super::inputs::#create_input_ident,
                headers: &[::cratestack::client_rust::HeaderPair<'_>],
            ) -> Result<super::models::#model_ident, ::cratestack::client_rust::ClientError> {
                self.runtime.post(#route_path, input, headers).await
            }

            pub async fn update(
                &self,
                id: &#primary_key_type,
                input: &super::inputs::#update_input_ident,
                headers: &[::cratestack::client_rust::HeaderPair<'_>],
            ) -> Result<super::models::#model_ident, ::cratestack::client_rust::ClientError> {
                self.runtime.patch(&format!("{}/{}", #route_path, id), input, headers).await
            }

            pub async fn delete(
                &self,
                id: &#primary_key_type,
                headers: &[::cratestack::client_rust::HeaderPair<'_>],
            ) -> Result<super::models::#model_ident, ::cratestack::client_rust::ClientError> {
                self.runtime.delete(&format!("{}/{}", #route_path, id), headers).await
            }
        }
    })
}
