//! `<Model>GrpcApi<T>` — one struct + 5 CRUD methods per model with a
//! primary key, mirroring what `tonic-build` itself emits for a client
//! stub (verified directly against `tonic-build-0.14.6`'s
//! `src/client.rs::generate_unary` and `tonic-0.13.1`'s own
//! `tonic::client::{Grpc, GrpcService}` — the version this workspace
//! pins). See [`super::client_struct`]'s doc for the outer `Client<T>`
//! this is accessed through (`client.widgets()`), and this module's own
//! differences from a raw `tonic-build` client:
//!
//! 1. One struct **per model**, not one flat struct with every method —
//!    matches the REST/RPC Rust clients' own `client.widgets().list(...)`
//!    shape (`crate::client::rpc::model`) so the call-site ergonomics
//!    don't change across transports.
//! 2. The repetitive `ready()` / path-building / codec-selection
//!    boilerplate `tonic-build` inlines into every method is factored into
//!    one shared helper, `CratestackGrpcClient::unary` in
//!    `cratestack-client-rust` — see that crate's `src/grpc/core.rs` for
//!    why (envelope-signing/auth needs one reviewable, testable
//!    implementation, not N copies across generated tokens).
//! 3. Errors are `::cratestack::client_rust::grpc::GrpcClientError`
//!    (wrapping `tonic::Status` directly — see that type's own doc for why
//!    there is no body to parse, unlike REST/RPC's JSON/CBOR error
//!    envelopes), not `tonic::Status` bare.
//!
//! Every method builds its request pb message, calls
//! `self.grpc.unary(method_name, request)`, and converts the pb response
//! back to the domain type via the same `TryFrom<pb::X> for X` conversions
//! `grpc_pb::message` already generates (bidirectional, shared with the
//! server) — no separate decode path to keep in sync.

use cratestack_core::{Field, Model};
use quote::quote;

use crate::include::grpc_pb::scalar::wire_from_domain_expr;
use crate::shared::ident;

use super::rpc_inputs::pk_domain_type;

/// One `<Model>GrpcApi<T>` struct + its CRUD methods.
pub(super) fn build_model_api(model: &Model, pk: &Field) -> proc_macro2::TokenStream {
    let model_name = &model.name;
    let model_ident = ident(model_name);
    let api_ident = ident(&format!("{model_name}GrpcApi"));
    let pk_input_ident = ident(&format!("{model_name}RpcPkInput"));
    let update_wrapper_ident = ident(&format!("{model_name}RpcUpdateInput"));
    let list_input_ident = ident(&format!("{model_name}RpcListInput"));
    let page_of_ident = ident(&format!("PageOf{model_name}"));
    let create_input_ident = ident(&format!("Create{model_name}Input"));
    let update_input_ident = ident(&format!("Update{model_name}Input"));

    let pk_type = pk_domain_type(pk);
    let pk_to_wire = wire_from_domain_expr(pk.ty.name.as_str(), quote! { id.clone() });

    let list_op = format!("model.{model_name}.list");
    let get_op = format!("model.{model_name}.get");
    let create_op = format!("model.{model_name}.create");
    let update_op = format!("model.{model_name}.update");
    let delete_op = format!("model.{model_name}.delete");
    let list_method = cratestack_proto::op_id_to_method_name(&list_op);
    let get_method = cratestack_proto::op_id_to_method_name(&get_op);
    let create_method = cratestack_proto::op_id_to_method_name(&create_op);
    let update_method = cratestack_proto::op_id_to_method_name(&update_op);
    let delete_method = cratestack_proto::op_id_to_method_name(&delete_op);

    let create_method_tokens = crate::include::grpc_pb::fields::model_allows_create(model).then(|| {
        quote! {
            /// `model.#model_name.create` — no envelope beyond the create
            /// input itself, same as REST/RPC.
            pub async fn create(
                &mut self,
                input: &super::inputs::#create_input_ident,
            ) -> ::core::result::Result<super::#model_ident, ::cratestack::client_rust::grpc::GrpcClientError> {
                let request = pb::#create_input_ident::from(input);
                let response: pb::#model_ident = self.grpc.unary(#create_method, request).await?;
                <super::#model_ident as ::core::convert::TryFrom<pb::#model_ident>>::try_from(response)
                    .map_err(::cratestack::client_rust::grpc::GrpcClientError::Codec)
            }
        }
    });

    quote! {
        #[derive(Debug, Clone)]
        pub struct #api_ident<T> {
            grpc: ::cratestack::client_rust::grpc::CratestackGrpcClient<T>,
        }

        impl<T> #api_ident<T>
        where
            T: ::cratestack::grpc::tonic::client::GrpcService<::cratestack::grpc::tonic::body::Body>,
            T::Error: ::core::convert::Into<::cratestack::grpc::tonic::codegen::StdError>,
            T::ResponseBody: ::cratestack::grpc::tonic::codegen::Body<Data = ::cratestack::grpc::tonic::codegen::Bytes>
                + ::core::marker::Send
                + 'static,
            <T::ResponseBody as ::cratestack::grpc::tonic::codegen::Body>::Error:
                ::core::convert::Into<::cratestack::grpc::tonic::codegen::StdError> + ::core::marker::Send,
        {
            /// `model.#model_name.list` — always returns `PageOf<Model>`
            /// on the wire regardless of the model's own `@@paged`
            /// attribute (`docs/design/protobuf.md`'s gRPC-specific rule,
            /// mirrored from `include::server::grpc::service::
            /// build_list_arm`'s own doc).
            pub async fn list(
                &mut self,
                input: &::cratestack::rpc::RpcListInput,
            ) -> ::core::result::Result<::cratestack::Page<super::#model_ident>, ::cratestack::client_rust::grpc::GrpcClientError>
            {
                let request = pb::#list_input_ident {
                    limit: input.limit,
                    offset: input.offset,
                    fields: input.fields.clone().unwrap_or_default(),
                    include: input.include.clone().unwrap_or_default(),
                    include_fields: input
                        .include_fields
                        .iter()
                        .map(|(key, values)| (key.clone(), pb::StringList { values: values.clone() }))
                        .collect(),
                    sort: input.sort.clone(),
                    where_expr: input.where_expr.clone(),
                    or: input.or.clone(),
                    filters: input
                        .filters
                        .iter()
                        .map(|predicate| pb::RpcListPredicate {
                            key: predicate.key.clone(),
                            value: predicate.value.clone(),
                        })
                        .collect(),
                };
                let response: pb::#page_of_ident = self.grpc.unary(#list_method, request).await?;
                let items = response
                    .items
                    .into_iter()
                    .map(<super::#model_ident as ::core::convert::TryFrom<pb::#model_ident>>::try_from)
                    .collect::<::core::result::Result<::std::vec::Vec<_>, ::cratestack::CoolError>>()
                    .map_err(::cratestack::client_rust::grpc::GrpcClientError::Codec)?;
                let page_info = response
                    .page_info
                    .map(|info| ::cratestack::PageInfo {
                        limit: info.limit,
                        offset: info.offset,
                        has_next_page: info.has_next_page,
                        has_previous_page: info.has_previous_page,
                    })
                    .unwrap_or_default();
                Ok(::cratestack::Page::new(items, page_info).with_total_count(response.total_count))
            }

            /// `model.#model_name.get` — wraps `id` in `<Model>RpcPkInput { id }`.
            pub async fn get(
                &mut self,
                id: &#pk_type,
            ) -> ::core::result::Result<super::#model_ident, ::cratestack::client_rust::grpc::GrpcClientError>
            {
                let request = pb::#pk_input_ident { id: Some(#pk_to_wire) };
                let response: pb::#model_ident = self.grpc.unary(#get_method, request).await?;
                <super::#model_ident as ::core::convert::TryFrom<pb::#model_ident>>::try_from(response)
                    .map_err(::cratestack::client_rust::grpc::GrpcClientError::Codec)
            }

            #create_method_tokens

            /// `model.#model_name.update` — wraps `id` + `patch` in
            /// `<Model>RpcUpdateInput { id, patch }`.
            pub async fn update(
                &mut self,
                id: &#pk_type,
                patch: &super::inputs::#update_input_ident,
            ) -> ::core::result::Result<super::#model_ident, ::cratestack::client_rust::grpc::GrpcClientError>
            {
                let request = pb::#update_wrapper_ident {
                    id: Some(#pk_to_wire),
                    patch: Some(Box::new(pb::#update_input_ident::from(patch))),
                };
                let response: pb::#model_ident = self.grpc.unary(#update_method, request).await?;
                <super::#model_ident as ::core::convert::TryFrom<pb::#model_ident>>::try_from(response)
                    .map_err(::cratestack::client_rust::grpc::GrpcClientError::Codec)
            }

            /// `model.#model_name.delete` — wraps `id` in `<Model>RpcPkInput { id }`.
            /// Returns the deleted record (same as REST DELETE).
            pub async fn delete(
                &mut self,
                id: &#pk_type,
            ) -> ::core::result::Result<super::#model_ident, ::cratestack::client_rust::grpc::GrpcClientError>
            {
                let request = pb::#pk_input_ident { id: Some(#pk_to_wire) };
                let response: pb::#model_ident = self.grpc.unary(#delete_method, request).await?;
                <super::#model_ident as ::core::convert::TryFrom<pb::#model_ident>>::try_from(response)
                    .map_err(::cratestack::client_rust::grpc::GrpcClientError::Codec)
            }
        }
    }
}
