//! RPC `ProceduresClient` method emission — split out of `client/rpc.rs`
//! per the repo's 200-LoC file convention (mirrors the sibling `model`
//! submodule's split for the per-model client). Unary procedures return
//! `BatchableCall<C, Output>`; `@stream`/bare-list procedures return
//! `RpcStream<Item>` instead (see the module doc on `client::rpc` for the
//! full rationale).

use cratestack_core::{Procedure, TypeArity};
use quote::quote;
use std::collections::BTreeSet;

use crate::computed::{ProcedureOutputComposition, procedure_output_composition};
use crate::procedure::procedure_client_output_item_tokens;
use crate::shared::{ident, to_snake_case};

/// `bearing`-driven output type — see `client::rest`'s twin function for
/// the full rationale; the RPC surface just wraps the same
/// `super::wire::<Owner>` substitution in its own `RpcStream<Item>` /
/// `BatchableCall<C, Output>` shapes.
pub(super) fn generate_generated_rpc_procedure_client_method(
    procedure: &Procedure,
    bearing: &BTreeSet<String>,
) -> Result<proc_macro2::TokenStream, String> {
    let method_ident = ident(&to_snake_case(&procedure.name));
    let module_ident = ident(&to_snake_case(&procedure.name));
    let op_id = format!("procedure.{}", procedure.name);

    let composition = procedure_output_composition(&procedure.return_type, bearing);

    // Sequence procedure → streaming. Return an `RpcStream<Item>` so
    // callers consume frames as they parse off the wire; the bounded
    // mpsc channel gives natural backpressure. `item_type` is the
    // sibling `wire::<Owner>` struct for a bearing item, otherwise the
    // exact tokens this function emitted before `@computed` existed.
    let list_item_type = match &composition {
        Some(ProcedureOutputComposition::List { owner }) => {
            let owner_ident = ident(owner);
            Some(quote! { super::wire::#owner_ident })
        }
        _ if matches!(procedure.return_type.arity, TypeArity::List) => {
            Some(procedure_client_output_item_tokens(&procedure.return_type))
        }
        _ => None,
    };
    if let Some(item_type) = list_item_type {
        return Ok(quote! {
            #[doc = concat!(
                "Streaming RPC call to `",
                #op_id,
                "`. Returns an `RpcStream<Item>` — a bounded `mpsc::Receiver` ",
                "that yields each cbor-seq item as it parses off the wire. ",
                "Non-2xx responses surface as `Err` from this call before the ",
                "channel ever opens; per-item failures appear as terminal `Err` ",
                "items on the channel."
            )]
            pub async fn #method_ident(
                &self,
                args: &super::procedures::#module_ident::Args,
            ) -> Result<
                ::cratestack::client_rust::RpcStream<#item_type>,
                ::cratestack::client_rust::RpcClientError,
            > {
                self.rpc
                    .call_streaming::<_, #item_type>(#op_id, args)
                    .await
            }
        });
    }

    // Unary procedure → BatchableCall. `.await` to fire immediately,
    // `.queue(&mut batch)` to defer into a `/rpc/batch` round-trip.
    let output_type = match composition {
        Some(ProcedureOutputComposition::Unary { owner, optional }) => {
            let owner_ident = ident(&owner);
            let base = quote! { super::wire::#owner_ident };
            if optional {
                quote! { Option<#base> }
            } else {
                base
            }
        }
        Some(ProcedureOutputComposition::Page { owner }) => {
            let owner_ident = ident(&owner);
            quote! { ::cratestack::Page<super::wire::#owner_ident> }
        }
        Some(ProcedureOutputComposition::List { .. }) => unreachable!(
            "handled by the early return above — a List composition always \
             takes the streaming branch"
        ),
        None => quote! { super::procedures::#module_ident::Output },
    };

    Ok(quote! {
        #[doc = concat!(
            "Unary RPC call to `",
            #op_id,
            "`. Returns a `BatchableCall` — `.await` to fire immediately, ",
            "or `.queue(&mut batch)` to defer."
        )]
        pub fn #method_ident(
            &self,
            args: &super::procedures::#module_ident::Args,
        ) -> ::cratestack::client_rust::BatchableCall<C, #output_type> {
            ::cratestack::client_rust::BatchableCall::new(
                self.rpc.clone(),
                #op_id,
                args,
            )
        }
    })
}
