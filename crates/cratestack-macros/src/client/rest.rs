//! REST client codegen (`transport rest`). Top-level `client::Client`
//! plus a per-model `<Model>Client` (in [`model`]) and a
//! `ProceduresClient`. All requests flow through
//! `CratestackClient::{get,post,patch,delete}`; codec is generic
//! (defaults to CBOR).

mod model;

use cratestack_core::{Model, Procedure};
use quote::quote;

use crate::procedure::procedure_client_output_item_tokens;
use crate::shared::{ident, pluralize, to_snake_case};

use model::generate_generated_model_client;

pub(super) fn generate_generated_client_module(
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

fn generate_generated_procedure_client_method(
    procedure: &Procedure,
) -> Result<proc_macro2::TokenStream, String> {
    let method_ident = ident(&to_snake_case(&procedure.name));
    let module_ident = ident(&to_snake_case(&procedure.name));
    let route_path = format!("/$procs/{}", procedure.name);
    let call = if matches!(
        procedure.return_type.arity,
        cratestack_core::TypeArity::List
    ) {
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
