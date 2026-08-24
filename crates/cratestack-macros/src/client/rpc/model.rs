//! RPC per-model client struct: 5 CRUD methods returning
//! `BatchableCall<C, Output>` so callers can either `.await` them or
//! `.queue(&mut batch)` into a multiplexed `/rpc/batch` round-trip.

use std::collections::BTreeSet;

use cratestack_core::Model;
use quote::quote;

use crate::client::model_output_type_tokens;
use crate::shared::{ident, is_paged_model, is_primary_key, rust_type_tokens};

mod computed;
mod view;
use computed::{build_get_method, build_list_method};
use view::build_get_view_method;

pub(super) fn generate_generated_rpc_model_client(
    model: &Model,
    bearing: &BTreeSet<String>,
    computed_params_ident: Option<&syn::Ident>,
) -> Result<proc_macro2::TokenStream, String> {
    let model_name = &model.name;
    let client_ident = ident(&format!("{}Client", model.name));
    let create_input_ident = ident(&format!("Create{}Input", model.name));
    let update_input_ident = ident(&format!("Update{}Input", model.name));

    let primary_key = model
        .fields
        .iter()
        .find(|field| is_primary_key(field))
        .ok_or_else(|| format!("model {} is missing a primary key", model.name))?;
    let primary_key_type = rust_type_tokens(&primary_key.ty);

    let model_output_type = model_output_type_tokens(&model.name, bearing);

    let paged = is_paged_model(model);
    let list_output_type = if paged {
        quote! { ::cratestack::Page<#model_output_type> }
    } else {
        quote! { Vec<#model_output_type> }
    };

    let list_op = format!("model.{model_name}.list");
    let get_op = format!("model.{model_name}.get");
    let create_op = format!("model.{model_name}.create");
    let update_op = format!("model.{model_name}.update");
    let delete_op = format!("model.{model_name}.delete");

    // `computed_params_ident` gates `list`/`get` on whether this model
    // declares at least one parameterized `@computed` field
    // (`crate::client::computed_params::model_computed_params_ident`) —
    // an ungated model keeps the exact tokens this function emitted
    // before this feature existed, INCLUDING `get`'s `RpcPkInput { id }`
    // shape: only a gated model's `get` switches to `RpcGetInput`, since
    // that's the only case with a `computedParams` value to carry.
    // Builders live in `model/computed.rs` (200-LoC file convention).
    let list_method = build_list_method(computed_params_ident, &list_op, &list_output_type);
    let get_method = build_get_method(
        computed_params_ident,
        &get_op,
        &primary_key_type,
        &model_output_type,
    );
    let get_view_method = build_get_view_method(&get_op, &primary_key_type);

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

            #list_method

            #get_method

            #get_view_method

            /// `POST /rpc/model.X.create` — body is the create input
            /// directly (no envelope; server delegates to the existing
            /// REST POST handler unchanged).
            pub fn create(
                &self,
                input: &super::inputs::#create_input_ident,
            ) -> ::cratestack::client_rust::BatchableCall<C, #model_output_type> {
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
            ) -> ::cratestack::client_rust::BatchableCall<C, #model_output_type> {
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
            ) -> ::cratestack::client_rust::BatchableCall<C, #model_output_type> {
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
