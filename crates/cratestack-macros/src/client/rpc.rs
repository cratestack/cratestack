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
mod procedure;

use std::collections::BTreeSet;

use cratestack_core::{Model, Procedure};
use quote::quote;

use crate::client::computed_params::{
    generate_model_computed_params_struct, model_computed_params_ident,
};
use crate::shared::{ident, pluralize, to_snake_case};

use model::generate_generated_rpc_model_client;
use procedure::generate_generated_rpc_procedure_client_method;

pub(super) fn generate_generated_rpc_client_module(
    models: &[Model],
    procedures: &[Procedure],
    bearing: &BTreeSet<String>,
) -> Result<proc_macro2::TokenStream, String> {
    let mut model_clients = Vec::new();
    for model in models {
        if let Some(computed_params_struct) = generate_model_computed_params_struct(model) {
            model_clients.push(computed_params_struct);
        }
        let computed_params_ident = model_computed_params_ident(model);
        model_clients.push(generate_generated_rpc_model_client(
            model,
            bearing,
            computed_params_ident.as_ref(),
        )?);
    }
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
        .map(|procedure| generate_generated_rpc_procedure_client_method(procedure, bearing))
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
                    // Issue #178: stamp this schema's SHA-256 onto the
                    // runtime before it's wrapped, so every RPC call
                    // (which goes through this same `CratestackClient`
                    // under the hood) carries `x-cratestack-schema-sha`.
                    Self {
                        rpc: ::cratestack::client_rust::RpcClient::new(
                            runtime.with_schema_sha(super::SCHEMA_SHA256),
                        ),
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
