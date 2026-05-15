use cratestack_core::{Model, Procedure, TransportStyle, TypeArity};
use quote::quote;

use crate::procedure::procedure_client_output_item_tokens;
use crate::shared::pluralize;
use crate::shared::{ident, is_paged_model, is_primary_key, rust_type_tokens, to_snake_case};

/// Top-level entry point — picks REST or RPC client codegen based on the
/// schema's `transport` directive. Both modes emit the same outer shape
/// (`cratestack_schema::client::Client`, per-model accessors, a
/// `procedures()` sub-client) so downstream call sites that read the
/// generated surface don't have to know which path was taken. The
/// methods on the inner per-model and procedure clients differ — see
/// the two impl functions below.
pub(crate) fn generate_client_module(
    models: &[Model],
    procedures: &[Procedure],
    transport: TransportStyle,
) -> Result<proc_macro2::TokenStream, String> {
    match transport {
        TransportStyle::Rest => generate_generated_client_module(models, procedures),
        TransportStyle::Rpc => generate_generated_rpc_client_module(models, procedures),
    }
}

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

// -----------------------------------------------------------------------------
// RPC client codegen (`transport rpc`)
//
// Same outer shape as the REST module (`Client`, per-model `XClient`,
// `ProceduresClient`) so consuming code doesn't change at the call site;
// the differences are all in the inner methods:
//
//   * Per-model: 5 CRUD methods that POST to `/rpc/model.X.{verb}` via
//     `RpcClient::call`. Input/output envelopes (`RpcListInput`,
//     `RpcPkInput`, `RpcUpdateInput`) are constructed inside the methods
//     so the user-facing API stays close to REST's — `get(id)` not
//     `get(RpcPkInput { id })`.
//
//   * Procedures: unary procedures hit `RpcClient::call`; list-return
//     procedures (`T[]`) hit `RpcClient::call_streaming` and return an
//     `RpcStream<Item>` (alias for `Receiver<Result<Item, RpcClientError>>`).
//
//   * Errors are `RpcClientError` (decoded from server `RpcErrorBody`)
//     instead of the REST `ClientError` shape, so call sites can switch
//     on the gRPC-style `code` string directly.
//
// `headers` and per-call options are dropped from the surface — `RpcClient`
// has no per-call header param today; auth flows via the underlying
// `CratestackClient::with_request_authorizer`. Adding per-call options
// is a future surface extension on `RpcClient` itself, not this codegen.
// -----------------------------------------------------------------------------

pub(crate) fn generate_generated_rpc_client_module(
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
                /// Build a typed RPC client from a configured `CratestackClient`.
                /// The `CratestackClient`'s `request_authorizer` (set via
                /// `.with_request_authorizer(...)`) flows through to every RPC
                /// call — auth headers, signing envelopes, etc.
                pub fn new(runtime: ::cratestack::client_rust::CratestackClient<C>) -> Self {
                    Self {
                        rpc: ::cratestack::client_rust::RpcClient::new(runtime),
                    }
                }

                /// Underlying `RpcClient`. Use for ops not covered by the
                /// typed surface (raw `call(op_id, &input)`, batch, etc.).
                pub fn rpc(&self) -> &::cratestack::client_rust::RpcClient<C> {
                    &self.rpc
                }

                /// Underlying REST client. Exposed for callers that need to
                /// reach the `CratestackClient` surface directly (state
                /// store, journal, etc.) without going through the RPC
                /// wrapper.
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

fn generate_generated_rpc_model_client(
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

fn generate_generated_rpc_procedure_client_method(
    procedure: &Procedure,
) -> Result<proc_macro2::TokenStream, String> {
    let method_ident = ident(&to_snake_case(&procedure.name));
    let module_ident = ident(&to_snake_case(&procedure.name));
    let op_id = format!("procedure.{}", procedure.name);

    if matches!(procedure.return_type.arity, TypeArity::List) {
        // Sequence procedure → streaming. Return an `RpcStream<Item>` so
        // callers consume frames as they parse off the wire; the bounded
        // mpsc channel gives natural backpressure.
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
        // Unary procedure → BatchableCall. `.await` to fire immediately,
        // `.queue(&mut batch)` to defer into a `/rpc/batch` round-trip.
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
