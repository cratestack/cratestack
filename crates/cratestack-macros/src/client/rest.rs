//! REST client codegen (`transport rest`). Top-level `client::Client`
//! plus a per-model `<Model>Client` (in [`model`]) and a
//! `ProceduresClient`. All requests flow through
//! `CratestackClient::{get,post,patch,delete}`; codec is generic
//! (defaults to CBOR).

mod model;

use std::collections::BTreeSet;

use cratestack_core::{Model, Procedure};
use quote::quote;

use crate::computed::{ProcedureOutputComposition, procedure_output_composition};
use crate::procedure::procedure_client_output_item_tokens;
use crate::shared::{ident, pluralize, to_snake_case};

use model::generate_generated_model_client;

pub(super) fn generate_generated_client_module(
    models: &[Model],
    procedures: &[Procedure],
    bearing: &BTreeSet<String>,
) -> Result<proc_macro2::TokenStream, String> {
    let model_accessors = models
        .iter()
        .map(|model| generate_generated_model_client(model, bearing))
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
        .map(|procedure| generate_generated_procedure_client_method(procedure, bearing))
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
                    // Issue #178: stamp this schema's SHA-256 onto the
                    // client automatically so every request carries
                    // `x-cratestack-schema-sha` — the schema author's own
                    // `CratestackClient::cbor(config)` call site needs no
                    // changes.
                    Self {
                        runtime: runtime.with_schema_sha(super::SCHEMA_SHA256),
                    }
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

/// `bearing`-driven output type: a procedure whose return type reaches a
/// computed-bearing owner (`procedure_output_composition`, `None` for
/// every procedure today except the ones this feature added coverage
/// for) decodes into the sibling `super::wire::<Owner>` struct instead of
/// `super::procedures::<name>::Output` — that `Output` alias is the
/// procedure's own server-side return type (`generate_procedure_module`),
/// which composition resolves computed fields *into* before encoding but
/// whose Rust type never carried them (`docs/design/computed-fields.md`'s
/// "Exclusions" section). Every other procedure keeps the exact tokens
/// this function emitted before `@computed` existed.
fn generate_generated_procedure_client_method(
    procedure: &Procedure,
    bearing: &BTreeSet<String>,
) -> Result<proc_macro2::TokenStream, String> {
    let method_ident = ident(&to_snake_case(&procedure.name));
    let module_ident = ident(&to_snake_case(&procedure.name));
    let route_path = format!("/$procs/{}", procedure.name);

    let (output_type, call) = match procedure_output_composition(&procedure.return_type, bearing) {
        Some(ProcedureOutputComposition::List { owner }) => {
            let owner_ident = ident(&owner);
            let item_type = quote! { super::wire::#owner_ident };
            (
                quote! { Vec<#item_type> },
                quote! { self.runtime.post_list::<_, #item_type>(#route_path, args, headers).await },
            )
        }
        Some(ProcedureOutputComposition::Unary { owner, optional }) => {
            let owner_ident = ident(&owner);
            let base = quote! { super::wire::#owner_ident };
            let output_type = if optional {
                quote! { Option<#base> }
            } else {
                base
            };
            (
                output_type,
                quote! { self.runtime.post(#route_path, args, headers).await },
            )
        }
        Some(ProcedureOutputComposition::Page { owner }) => {
            let owner_ident = ident(&owner);
            (
                quote! { ::cratestack::Page<super::wire::#owner_ident> },
                quote! { self.runtime.post(#route_path, args, headers).await },
            )
        }
        None => {
            let call = if matches!(
                procedure.return_type.arity,
                cratestack_core::TypeArity::List
            ) {
                let item_type = procedure_client_output_item_tokens(&procedure.return_type);
                quote! { self.runtime.post_list::<_, #item_type>(#route_path, args, headers).await }
            } else {
                quote! { self.runtime.post(#route_path, args, headers).await }
            };
            (quote! { super::procedures::#module_ident::Output }, call)
        }
    };

    Ok(quote! {
        pub async fn #method_ident(
            &self,
            args: &super::procedures::#module_ident::Args,
            headers: &[::cratestack::client_rust::HeaderPair<'_>],
        ) -> Result<#output_type, ::cratestack::client_rust::ClientError> {
            #call
        }
    })
}
