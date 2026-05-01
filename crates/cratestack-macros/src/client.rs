use cratestack_core::{Model, Procedure};
use quote::quote;

use crate::procedure::procedure_client_output_item_tokens;
use crate::shared::pluralize;
use crate::shared::{ident, is_paged_model, is_primary_key, rust_type_tokens, to_snake_case};

pub(crate) fn generate_generated_client_module(
    models: &[Model],
    procedures: &[Procedure],
) -> Result<proc_macro2::TokenStream, String> {
    let model_accessors = models
        .iter()
        .map(generate_generated_model_client)
        .collect::<Result<Vec<_>, String>>()?;
    let model_client_accessors = models
        .iter()
        .map(|model| {
            let method_ident = ident(&pluralize(&to_snake_case(&model.name)));
            let client_ident = ident(&format!("{}Client", model.name));
            quote! {
                pub fn #method_ident(&self) -> #client_ident<C> {
                    #client_ident::new(self.runtime.clone())
                }
            }
        })
        .collect::<Vec<_>>();
    let procedure_methods = procedures
        .iter()
        .map(generate_generated_procedure_client_method)
        .collect::<Result<Vec<_>, String>>()?;

    Ok(quote! {
        pub mod client {
            #[derive(Clone)]
            pub struct Client<C = ::cratestack::client_rust::CborCodec>
            where
                C: ::cratestack::client_rust::HttpClientCodec,
            {
                runtime: ::cratestack::client_rust::CratestackClient<C>,
            }

            impl<C> Client<C>
            where
                C: ::cratestack::client_rust::HttpClientCodec,
            {
                pub fn new(runtime: ::cratestack::client_rust::CratestackClient<C>) -> Self {
                    Self { runtime }
                }

                pub fn runtime(&self) -> &::cratestack::client_rust::CratestackClient<C> {
                    &self.runtime
                }

                #(#model_client_accessors)*

                pub fn procedures(&self) -> ProceduresClient<C> {
                    ProceduresClient::new(self.runtime.clone())
                }
            }

            #(#model_accessors)*

            #[derive(Clone)]
            pub struct ProceduresClient<C = ::cratestack::client_rust::CborCodec>
            where
                C: ::cratestack::client_rust::HttpClientCodec,
            {
                runtime: ::cratestack::client_rust::CratestackClient<C>,
            }

            impl<C> ProceduresClient<C>
            where
                C: ::cratestack::client_rust::HttpClientCodec,
            {
                fn new(runtime: ::cratestack::client_rust::CratestackClient<C>) -> Self {
                    Self { runtime }
                }

                #(#procedure_methods)*
            }
        }
    })
}

fn generate_generated_model_client(model: &Model) -> Result<proc_macro2::TokenStream, String> {
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
    let list_call = if paged {
        quote! { self.runtime.get(#route_path, query, headers).await }
    } else {
        quote! { self.runtime.get(#route_path, query, headers).await }
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
                #list_call
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

fn generate_generated_procedure_client_method(
    procedure: &Procedure,
) -> Result<proc_macro2::TokenStream, String> {
    let method_ident = ident(&to_snake_case(&procedure.name));
    let module_ident = ident(&to_snake_case(&procedure.name));
    let route_path = format!("/$procs/{}", procedure.name);
    let call = if matches!(procedure.return_type.arity, cratestack_core::TypeArity::List) {
        let item_type = procedure_client_output_item_tokens(&procedure.return_type);
        quote! { self.runtime.post_list::<_, #item_type>(#route_path, args, headers).await }
    } else {
        quote! { self.runtime.post(#route_path, args, headers).await }
    };

    Ok(quote! {
        pub async fn #method_ident(
            &self,
            args: &super::procedures::#module_ident::Args,
            headers: &[::cratestack::client_rust::HeaderPair<'_>],
        ) -> Result<super::procedures::#module_ident::Output, ::cratestack::client_rust::ClientError> {
            #call
        }
    })
}
