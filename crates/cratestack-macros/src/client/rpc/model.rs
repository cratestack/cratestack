//! RPC per-model client struct: 5 CRUD methods returning
//! `BatchableCall<C, Output>` so callers can either `.await` them or
//! `.queue(&mut batch)` into a multiplexed `/rpc/batch` round-trip.

use cratestack_core::Model;
use quote::quote;

use crate::shared::{ident, is_paged_model, is_primary_key, rust_type_tokens};

pub(super) fn generate_generated_rpc_model_client(
    model: &Model,
) -> Result<proc_macro2::TokenStream, String> {
    let model_name = &model.name;
    let client_ident = ident(&format!("{}Client", model.name));
    let model_ident = ident(&model.name);
    let create_input_ident = ident(&format!("Create{}Input", model.name));
    let update_input_ident = ident(&format!("Update{}Input", model.name));

    let primary_key = model
        .fields
        .iter()
        .find(|field| is_primary_key(field))
        .ok_or_else(|| format!("model {} is missing a primary key", model.name))?;
    let primary_key_type = rust_type_tokens(&primary_key.ty);

    let paged = is_paged_model(model);
    let list_output_type = if paged {
        quote! { ::cratestack::Page<super::models::#model_ident> }
    } else {
        quote! { Vec<super::models::#model_ident> }
    };

    let list_op = format!("model.{model_name}.list");
    let get_op = format!("model.{model_name}.get");
    let create_op = format!("model.{model_name}.create");
    let update_op = format!("model.{model_name}.update");
    let delete_op = format!("model.{model_name}.delete");

    Ok(quote! {
        #[derive(Clone)]
        pub struct #client_ident<C = ::cratestack::client_rust::CborCodec>
        where
            C: ::cratestack::client_rust::HttpClientCodec + Clone,
        {
            rpc: ::cratestack::client_rust::RpcClient<C>,
        }

        impl<C> #client_ident<C>
        where
            C: ::cratestack::client_rust::HttpClientCodec + Clone + Send + 'static,
        {
            fn new(rpc: ::cratestack::client_rust::RpcClient<C>) -> Self {
                Self { rpc }
            }

            /// `POST /rpc/model.X.list` — server decodes `RpcListInput`,
            /// synthesizes a query string, and runs the same list
            /// handler as the REST binding. Output shape is unchanged:
            /// paged models return `Page<Model>`, non-paged return
            /// `Vec<Model>`.
            ///
            /// Returns a [`BatchableCall`](::cratestack::client_rust::BatchableCall)
            /// — `.await` to fire immediately, or
            /// `.queue(&mut batch)` to defer into a multiplexed
            /// `/rpc/batch` round-trip.
            pub fn list(
                &self,
                input: &::cratestack::rpc::RpcListInput,
            ) -> ::cratestack::client_rust::BatchableCall<C, #list_output_type> {
                ::cratestack::client_rust::BatchableCall::new(
                    self.rpc.clone(),
                    #list_op,
                    input,
                )
            }

            /// `POST /rpc/model.X.get` — wraps `id` in `RpcPkInput { id }`.
            pub fn get(
                &self,
                id: &#primary_key_type,
            ) -> ::cratestack::client_rust::BatchableCall<C, super::models::#model_ident> {
                let input = ::cratestack::rpc::RpcPkInput {
                    id: id.clone(),
                };
                ::cratestack::client_rust::BatchableCall::new(
                    self.rpc.clone(),
                    #get_op,
                    &input,
                )
            }

            /// `POST /rpc/model.X.create` — body is the create input
            /// directly (no envelope; server delegates to the existing
            /// REST POST handler unchanged).
            pub fn create(
                &self,
                input: &super::inputs::#create_input_ident,
            ) -> ::cratestack::client_rust::BatchableCall<C, super::models::#model_ident> {
                ::cratestack::client_rust::BatchableCall::new(
                    self.rpc.clone(),
                    #create_op,
                    input,
                )
            }

            /// `POST /rpc/model.X.update` — wraps `id` + `patch` in
            /// `RpcUpdateInput { id, patch }`. The patch is the same
            /// `Update<Model>Input` struct as the REST PATCH body, so
            /// `Option::None` round-trips through CBOR correctly.
            pub fn update(
                &self,
                id: &#primary_key_type,
                patch: &super::inputs::#update_input_ident,
            ) -> ::cratestack::client_rust::BatchableCall<C, super::models::#model_ident> {
                let input = ::cratestack::rpc::RpcUpdateInput {
                    id: id.clone(),
                    patch: patch.clone(),
                };
                ::cratestack::client_rust::BatchableCall::new(
                    self.rpc.clone(),
                    #update_op,
                    &input,
                )
            }

            /// `POST /rpc/model.X.delete` — wraps `id` in `RpcPkInput { id }`.
            /// Returns the deleted record (same as REST DELETE).
            pub fn delete(
                &self,
                id: &#primary_key_type,
            ) -> ::cratestack::client_rust::BatchableCall<C, super::models::#model_ident> {
                let input = ::cratestack::rpc::RpcPkInput {
                    id: id.clone(),
                };
                ::cratestack::client_rust::BatchableCall::new(
                    self.rpc.clone(),
                    #delete_op,
                    &input,
                )
            }
        }
    })
}
