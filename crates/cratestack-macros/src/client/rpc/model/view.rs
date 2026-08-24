//! `get_view` method builder for the RPC per-model client, split out
//! of `client/rpc/model.rs` per the repo's 200-LoC file convention.
//!
//! `pub async fn get_view<P>(...)` — the RPC twin of the REST client's
//! `get_view` (`crate::client::rest::model`). Emitted for EVERY model,
//! gated or not: `fields`/`include`/`includeFields` have nothing to do
//! with `@computed`, and `RpcGetInput` decodes them regardless.
//!
//! Not a `BatchableCall`: the projected payload decodes through
//! `ProjectionDecoder`, not `serde::DeserializeOwned`, so it can't ride
//! `BatchableCall<C, O>`'s bound. Batching a projected get is still
//! possible at the raw-frame level (`RpcClient::batch`) — the server
//! supports it per-frame.
//!
//! Carries no `computedParams`, matching REST's `get_view` exactly
//! (`cratestack-client-rust/src/client/views.rs` passes `&[]` as its
//! extra query). Parity, not an oversight.

use proc_macro2::TokenStream;
use quote::quote;

pub(super) fn build_get_view_method(get_op: &str, primary_key_type: &TokenStream) -> TokenStream {
    quote! {
        pub async fn get_view<P>(
            &self,
            id: &#primary_key_type,
            projection: &P,
        ) -> Result<P::Output, ::cratestack::client_rust::RpcClientError>
        where
            P: ::cratestack::ProjectionDecoder,
        {
            let selection = ::cratestack::ProjectionDecoder::selection_query(projection);
            let input = ::cratestack::rpc::RpcGetInput {
                id: id.clone(),
                fields: if selection.fields.is_empty() {
                    ::core::option::Option::None
                } else {
                    ::core::option::Option::Some(selection.fields.clone())
                },
                include: if selection.includes.is_empty() {
                    ::core::option::Option::None
                } else {
                    ::core::option::Option::Some(selection.includes.clone())
                },
                include_fields: selection.include_fields.clone(),
                computed_params: ::core::option::Option::None,
            };
            let value: ::cratestack::serde_json::Value =
                self.rpc.call(#get_op, &input).await?;
            ::cratestack::ProjectionDecoder::decode_one(projection, value)
                .map_err(::cratestack::client_rust::RpcClientError::from)
        }
    }
}
