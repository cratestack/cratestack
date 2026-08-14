//! The two per-procedure `match` arms `service::build_service` splices
//! into `ApiServer::call` (ticket #208) — split out of `service.rs` to
//! stay under this repo's 200-LoC file convention. See `service.rs`'s
//! module doc for what "server-streaming" means for the streaming arm,
//! and `crud_arms::build_create_arm` for the CRUD sibling this unary
//! shape mirrors almost exactly (decode -> encode -> dispatch -> bridge).

use cratestack_core::Procedure;
use quote::quote;

use crate::shared::{ident, to_snake_case};

use super::arm_support::{request_prelude, status_from_bridge_error};

fn procedure_method_path(package: &str, procedure: &Procedure) -> String {
    let op_id = format!("procedure.{}", procedure.name);
    format!(
        "/{package}.Api/{}",
        cratestack_proto::op_id_to_method_name(&op_id)
    )
}

/// Unary procedure arm: decode the pb `<Base>Input`, convert it to
/// `procedures::<name>::Args` via the `TryFrom` impl
/// [`super::procedures::render_procedure_input`] generates, re-encode
/// through the schema's own wire codec, and call the exact same
/// `super::axum::handle_<name>_dispatch` fn REST/RPC already call.
pub(super) fn build_procedure_unary_arm(
    package: &str,
    procedure: &Procedure,
) -> proc_macro2::TokenStream {
    let path = procedure_method_path(package, procedure);
    let dispatch_ident = ident(&format!(
        "handle_{}_dispatch",
        to_snake_case(&procedure.name)
    ));
    let module_ident = ident(&to_snake_case(&procedure.name));
    let base = cratestack_proto::to_pascal_case(&procedure.name);
    let request_ty = ident(&format!("{base}Input"));
    let response_ty = ident(&format!("{base}Output"));
    let svc_ident = ident(&format!("Grpc{base}Svc"));
    let prelude = request_prelude(&path);
    let status = status_from_bridge_error(quote! { code }, quote! { message });

    quote! {
        #path => {
            struct #svc_ident<R, C, Auth>(super::axum::ProcedureRouterState<R, C, Auth>, ::cratestack::ClientIpContext);
            impl<R, C, Auth> ::cratestack::grpc::tonic::server::UnaryService<pb::#request_ty> for #svc_ident<R, C, Auth>
            where
                R: super::procedures::ProcedureRegistry,
                C: ::cratestack::HttpTransport + Send + Sync + 'static,
                Auth: ::cratestack::AuthProvider + Send + Sync + 'static,
            {
                type Response = pb::#response_ty;
                type Future = ::cratestack::grpc::tonic::codegen::BoxFuture<
                    ::cratestack::grpc::tonic::Response<Self::Response>,
                    ::cratestack::grpc::tonic::Status,
                >;
                fn call(&mut self, request: ::cratestack::grpc::tonic::Request<pb::#request_ty>) -> Self::Future {
                    let state = self.0.clone();
                    let client_ip_ctx = self.1.clone();
                    Box::pin(async move {
                        #prelude
                        let args: ::core::result::Result<super::procedures::#module_ident::Args, ::cratestack::CratestackError> =
                            super::procedures::#module_ident::Args::try_from(message);
                        let args = match args {
                            Ok(value) => value,
                            Err(error) => {
                                return Err(::cratestack::grpc::tonic::Status::new(
                                    ::cratestack::grpc::cratestack_error_code_to_tonic_code(error.code()),
                                    error.public_message().into_owned(),
                                ));
                            }
                        };
                        let body_bytes = match ::cratestack::__private::encode_rpc_value(&state.codec, &headers, &args).await {
                            Ok(bytes) => ::cratestack::axum::body::Bytes::from(bytes),
                            Err(error) => {
                                return Err(::cratestack::grpc::tonic::Status::new(
                                    ::cratestack::grpc::cratestack_error_code_to_tonic_code(error.code()),
                                    error.public_message().into_owned(),
                                ));
                            }
                        };
                        let response = super::axum::#dispatch_ident(state.clone(), canonical, headers.clone(), client_ip_ctx, body_bytes).await;
                        let domain: super::procedures::#module_ident::Output = match ::cratestack::__private::bridge_grpc_response(response, &state.codec, &headers).await {
                            Ok(value) => value,
                            Err((code, message)) => return Err(#status),
                        };
                        Ok(::cratestack::grpc::tonic::Response::new(pb::#response_ty::from(&domain)))
                    })
                }
            }
            let svc = #svc_ident(state, client_ip_ctx);
            let codec = ::cratestack::grpc::tonic::codec::ProstCodec::default();
            let mut grpc = ::cratestack::grpc::tonic::server::Grpc::new(codec);
            Box::pin(async move { Ok(grpc.unary(svc, req).await) })
        }
    }
}

/// Server-streaming procedure arm (`OpKind::Sequence` — `List`-arity
/// return): same decode/dispatch/bridge shape as
/// [`build_procedure_unary_arm`], but the bridged domain value is a
/// `Vec<Item>` (a `List`-arity procedure's `Output` type alias — see
/// `crate::procedure::types::procedure_type_tokens` — already resolves
/// to `Vec<Item>` directly, not a wrapper struct) and the one resulting
/// `<Base>Output { repeated result }` message travels back as a
/// single-item `tonic::server::ServerStreamingService` stream rather
/// than a unary response — see `service.rs`'s module doc for exactly
/// what that does and does not mean.
pub(super) fn build_procedure_stream_arm(
    package: &str,
    procedure: &Procedure,
) -> proc_macro2::TokenStream {
    let path = procedure_method_path(package, procedure);
    let dispatch_ident = ident(&format!(
        "handle_{}_dispatch",
        to_snake_case(&procedure.name)
    ));
    let module_ident = ident(&to_snake_case(&procedure.name));
    let base = cratestack_proto::to_pascal_case(&procedure.name);
    let request_ty = ident(&format!("{base}Input"));
    let response_ty = ident(&format!("{base}Output"));
    let svc_ident = ident(&format!("Grpc{base}StreamSvc"));
    let prelude = request_prelude(&path);
    let status = status_from_bridge_error(quote! { code }, quote! { message });

    quote! {
        #path => {
            struct #svc_ident<R, C, Auth>(super::axum::ProcedureRouterState<R, C, Auth>, ::cratestack::ClientIpContext);
            impl<R, C, Auth> ::cratestack::grpc::tonic::server::ServerStreamingService<pb::#request_ty> for #svc_ident<R, C, Auth>
            where
                R: super::procedures::ProcedureRegistry,
                C: ::cratestack::HttpTransport + Send + Sync + 'static,
                Auth: ::cratestack::AuthProvider + Send + Sync + 'static,
            {
                type Response = pb::#response_ty;
                type ResponseStream = ::cratestack::grpc::tonic::codegen::BoxStream<Self::Response>;
                type Future = ::cratestack::grpc::tonic::codegen::BoxFuture<
                    ::cratestack::grpc::tonic::Response<Self::ResponseStream>,
                    ::cratestack::grpc::tonic::Status,
                >;
                fn call(&mut self, request: ::cratestack::grpc::tonic::Request<pb::#request_ty>) -> Self::Future {
                    let state = self.0.clone();
                    let client_ip_ctx = self.1.clone();
                    Box::pin(async move {
                        #prelude
                        let args: ::core::result::Result<super::procedures::#module_ident::Args, ::cratestack::CratestackError> =
                            super::procedures::#module_ident::Args::try_from(message);
                        let args = match args {
                            Ok(value) => value,
                            Err(error) => {
                                return Err(::cratestack::grpc::tonic::Status::new(
                                    ::cratestack::grpc::cratestack_error_code_to_tonic_code(error.code()),
                                    error.public_message().into_owned(),
                                ));
                            }
                        };
                        let body_bytes = match ::cratestack::__private::encode_rpc_value(&state.codec, &headers, &args).await {
                            Ok(bytes) => ::cratestack::axum::body::Bytes::from(bytes),
                            Err(error) => {
                                return Err(::cratestack::grpc::tonic::Status::new(
                                    ::cratestack::grpc::cratestack_error_code_to_tonic_code(error.code()),
                                    error.public_message().into_owned(),
                                ));
                            }
                        };
                        let response = super::axum::#dispatch_ident(state.clone(), canonical, headers.clone(), client_ip_ctx, body_bytes).await;
                        let items: super::procedures::#module_ident::Output = match ::cratestack::__private::bridge_grpc_response(response, &state.codec, &headers).await {
                            Ok(value) => value,
                            Err((code, message)) => return Err(#status),
                        };
                        let wire = pb::#response_ty::from(&items);
                        let stream: Self::ResponseStream = Box::pin(
                            ::cratestack::grpc::tonic::codegen::tokio_stream::once(Ok(wire)),
                        );
                        Ok(::cratestack::grpc::tonic::Response::new(stream))
                    })
                }
            }
            let svc = #svc_ident(state, client_ip_ctx);
            let codec = ::cratestack::grpc::tonic::codec::ProstCodec::default();
            let mut grpc = ::cratestack::grpc::tonic::server::Grpc::new(codec);
            Box::pin(async move { Ok(grpc.server_streaming(svc, req).await) })
        }
    }
}
