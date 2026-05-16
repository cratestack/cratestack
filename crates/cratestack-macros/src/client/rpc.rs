//! RPC client codegen (`transport rpc`). Same outer shape as the REST
//! module (`Client`, per-model `XClient`, `ProceduresClient`) so
//! consuming code doesn't change at the call site; the differences are
//! all in the inner methods:
//!
//!   * Per-model: 5 CRUD methods that POST to `/rpc/model.X.{verb}` via
//!     `RpcClient::call`. Input/output envelopes (`RpcListInput`,
//!     `RpcPkInput`, `RpcUpdateInput`) are constructed inside the
//!     methods so the user-facing API stays close to REST's —
//!     `get(id)` not `get(RpcPkInput { id })`.
//!
//!   * Procedures: unary procedures hit `RpcClient::call`; list-return
//!     procedures (`T[]`) hit `RpcClient::call_streaming` and return an
//!     `RpcStream<Item>` (alias for
//!     `Receiver<Result<Item, RpcClientError>>`).
//!
//!   * Errors are `RpcClientError` (decoded from server `RpcErrorBody`)
//!     instead of the REST `ClientError` shape, so call sites can
//!     switch on the gRPC-style `code` string directly.
//!
//! `headers` and per-call options are dropped from the surface —
//! `RpcClient` has no per-call header param today; auth flows via
//! `CratestackClient::with_request_authorizer`.

mod model;

use cratestack_core::{Model, Procedure, TypeArity};
use quote::quote;

use crate::procedure::procedure_client_output_item_tokens;
use crate::shared::{ident, pluralize, to_snake_case};

use model::generate_generated_rpc_model_client;

pub(super) fn generate_generated_rpc_client_module(
    models: &[Model],
    procedures: &[Procedure],
) -> Result<proc_macro2::TokenStream, String> {
    let model_clients = models
        .iter()
        .map(generate_generated_rpc_model_client)
        .collect::<Result<Vec<_>, String>>()?;
    let model_client_accessors = models
        .iter()
        .map(|model| {
            let method_ident = ident(&pluralize(&to_snake_case(&model.name)));
            let client_ident = ident(&format!("{}Client", model.name));
            quote! {
                pub fn #method_ident(&self) -> #client_ident<C> {
                    #client_ident::new(self.rpc.clone())
                }
            }
        })
        .collect::<Vec<_>>();
    let procedure_methods = procedures
        .iter()
        .map(generate_generated_rpc_procedure_client_method)
        .collect::<Result<Vec<_>, String>>()?;

    Ok(quote! {
        pub mod client {
            #[derive(Clone)]
            pub struct Client<C = ::cratestack::client_rust::CborCodec>
            where
                C: ::cratestack::client_rust::HttpClientCodec + Clone,
            {
                rpc: ::cratestack::client_rust::RpcClient<C>,
            }

            impl<C> Client<C>
            where
                C: ::cratestack::client_rust::HttpClientCodec + Clone + Send + 'static,
            {
                /// Build a typed RPC client from a configured
                /// `CratestackClient`. The `CratestackClient`'s
                /// `request_authorizer` (set via
                /// `.with_request_authorizer(...)`) flows through to
                /// every RPC call — auth headers, signing envelopes, etc.
                pub fn new(runtime: ::cratestack::client_rust::CratestackClient<C>) -> Self {
                    Self {
                        rpc: ::cratestack::client_rust::RpcClient::new(runtime),
                    }
                }

                /// Underlying `RpcClient`. Use for ops not covered by
                /// the typed surface (raw `call(op_id, &input)`, batch,
                /// etc.).
                pub fn rpc(&self) -> &::cratestack::client_rust::RpcClient<C> {
                    &self.rpc
                }

                /// Underlying REST client. Exposed for callers that
                /// need to reach the `CratestackClient` surface
                /// directly (state store, journal, etc.) without going
                /// through the RPC wrapper.
                pub fn runtime(&self) -> &::cratestack::client_rust::CratestackClient<C> {
                    self.rpc.inner()
                }

                /// Start a typed batch. Chain `.queue(&mut batch)` from
                /// any unary RPC call on this client (model CRUD or
                /// procedure) to defer it into one `POST /rpc/batch`
                /// round-trip, then `batch.send().await` to fire.
                pub fn batch(&self) -> ::cratestack::client_rust::BatchBuilder<C> {
                    self.rpc.batch_builder()
                }

                #(#model_client_accessors)*

                pub fn procedures(&self) -> ProceduresClient<C> {
                    ProceduresClient::new(self.rpc.clone())
                }
            }

            #(#model_clients)*

            #[derive(Clone)]
            pub struct ProceduresClient<C = ::cratestack::client_rust::CborCodec>
            where
                C: ::cratestack::client_rust::HttpClientCodec + Clone,
            {
                rpc: ::cratestack::client_rust::RpcClient<C>,
            }

            impl<C> ProceduresClient<C>
            where
                C: ::cratestack::client_rust::HttpClientCodec + Clone + Send + 'static,
            {
                fn new(rpc: ::cratestack::client_rust::RpcClient<C>) -> Self {
                    Self { rpc }
                }

                #(#procedure_methods)*
            }
        }
    })
}

fn generate_generated_rpc_procedure_client_method(
    procedure: &Procedure,
) -> Result<proc_macro2::TokenStream, String> {
    let method_ident = ident(&to_snake_case(&procedure.name));
    let module_ident = ident(&to_snake_case(&procedure.name));
    let op_id = format!("procedure.{}", procedure.name);

    if matches!(procedure.return_type.arity, TypeArity::List) {
        // Sequence procedure → streaming. Return an `RpcStream<Item>`
        // so callers consume frames as they parse off the wire; the
        // bounded mpsc channel gives natural backpressure.
        let item_type = procedure_client_output_item_tokens(&procedure.return_type);
        Ok(quote! {
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
        })
    } else {
        // Unary procedure → BatchableCall. `.await` to fire
        // immediately, `.queue(&mut batch)` to defer into a
        // `/rpc/batch` round-trip.
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
            ) -> ::cratestack::client_rust::BatchableCall<
                C,
                super::procedures::#module_ident::Output,
            > {
                ::cratestack::client_rust::BatchableCall::new(
                    self.rpc.clone(),
                    #op_id,
                    args,
                )
            }
        })
    }
}
