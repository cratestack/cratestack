//! `list`/`get` method builders for the RPC per-model client, split out
//! of `client/rpc/model.rs` per the repo's 200-LoC file convention. Both
//! methods are gated on `computed_params_ident`
//! (`crate::client::computed_params::model_computed_params_ident`) — an
//! ungated model (`None`) keeps the exact tokens this function emitted
//! before the typed `computedParams` surface existed
//! (`docs/design/computed-fields.md`'s "Downstream" section), INCLUDING
//! `get`'s `RpcPkInput { id }` shape: only a gated model's `get` switches
//! to `RpcGetInput`, since that's the only case with a `computedParams`
//! value to carry.

use proc_macro2::TokenStream;
use quote::quote;

/// `pub fn list(...) -> BatchableCall<C, ListOutput>` — with a
/// `computed_params` parameter that overwrites `input`'s own
/// `computed_params` field when `computed_params_ident` is `Some`, or the
/// original ungated signature otherwise.
pub(super) fn build_list_method(
    computed_params_ident: Option<&syn::Ident>,
    list_op: &str,
    list_output_type: &TokenStream,
) -> TokenStream {
    match computed_params_ident {
        Some(computed_params_ident) => quote! {
            /// `POST /rpc/model.X.list` — server decodes `RpcListInput`,
            /// synthesizes a query string, and runs the same list
            /// handler as the REST binding. Output shape is unchanged:
            /// paged models return `Page<Model>`, non-paged return
            /// `Vec<Model>`.
            ///
            /// `computed_params`, when `Some`, overwrites `input`'s own
            /// `computed_params` field with the typed struct's encoded
            /// value — pass `None` to use whatever `input.computed_params`
            /// already carries (e.g. a raw hand-built value).
            ///
            /// Returns a [`BatchableCall`](::cratestack::client_rust::BatchableCall)
            /// — `.await` to fire immediately, or
            /// `.queue(&mut batch)` to defer into a multiplexed
            /// `/rpc/batch` round-trip.
            pub fn list(
                &self,
                input: &::cratestack::rpc::RpcListInput,
                computed_params: ::core::option::Option<&#computed_params_ident>,
            ) -> ::cratestack::client_rust::BatchableCall<C, #list_output_type> {
                let mut input = input.clone();
                if let Some(params) = computed_params {
                    input.computed_params = params.to_query_value();
                }
                ::cratestack::client_rust::BatchableCall::new(
                    self.rpc.clone(),
                    #list_op,
                    &input,
                )
            }
        },
        None => quote! {
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
        },
    }
}

/// `pub fn get(...) -> BatchableCall<C, ModelOutput>` — wraps `id` (and,
/// when `computed_params_ident` is `Some`, the typed `computed_params`'
/// encoded value) in `RpcGetInput`, or `RpcPkInput` for an ungated model.
pub(super) fn build_get_method(
    computed_params_ident: Option<&syn::Ident>,
    get_op: &str,
    primary_key_type: &TokenStream,
    model_output_type: &TokenStream,
) -> TokenStream {
    match computed_params_ident {
        Some(computed_params_ident) => quote! {
            /// `POST /rpc/model.X.get` — wraps `id` and the typed
            /// `computed_params`' encoded value in `RpcGetInput { id,
            /// computed_params }` (not `RpcPkInput`, which `delete` also
            /// decodes — see `RpcGetInput`'s own doc for why the two
            /// aren't shared).
            pub fn get(
                &self,
                id: &#primary_key_type,
                computed_params: ::core::option::Option<&#computed_params_ident>,
            ) -> ::cratestack::client_rust::BatchableCall<C, #model_output_type> {
                let input = ::cratestack::rpc::RpcGetInput {
                    id: id.clone(),
                    computed_params: computed_params.and_then(|params| params.to_query_value()),
                };
                ::cratestack::client_rust::BatchableCall::new(
                    self.rpc.clone(),
                    #get_op,
                    &input,
                )
            }
        },
        None => quote! {
            /// `POST /rpc/model.X.get` — wraps `id` in `RpcPkInput { id }`.
            pub fn get(
                &self,
                id: &#primary_key_type,
            ) -> ::cratestack::client_rust::BatchableCall<C, #model_output_type> {
                let input = ::cratestack::rpc::RpcPkInput {
                    id: id.clone(),
                };
                ::cratestack::client_rust::BatchableCall::new(
                    self.rpc.clone(),
                    #get_op,
                    &input,
                )
            }
        },
    }
}
