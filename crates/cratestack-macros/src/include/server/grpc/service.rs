//! The hand-rolled tonic service — no `tonic-build`/`protoc` involved at
//! macro-expansion time (consistent with `message.rs`'s mirror-struct
//! approach: this crate never shells out to `protoc`). The shape below
//! mirrors what `tonic-build` itself emits for a `service { ... }` block
//! (verified directly against `tonic-0.13.1`'s own source —
//! `tonic::server::{Grpc, UnaryService, ServerStreamingService}`,
//! `tonic::codec::ProstCodec`, `tonic::service::Routes` — the same
//! `axum::Router` this workspace already depends on, confirmed aligned by
//! `cargo tree`, closing ticket #171's first acceptance criterion).
//!
//! Each method arm: decode the tonic request's pb message, build exactly
//! the arguments the existing `super::axum::handle_*_dispatch` fn takes
//! (constructing a `CanonicalRequest` whose `path` is the gRPC method path
//! and whose `body` is the pb message re-encoded to bytes — see the
//! "Known gap" note below), call it, and bridge the resulting
//! `axum::Response` back through `cratestack_axum::rpc::bridge_grpc_response`
//! into either the pb response or a `tonic::Status`.
//!
//! **Known gap, flagged rather than hidden:** `docs/design/protobuf.md`
//! §7.3 specifies envelope signing over the *literal unframed wire bytes*.
//! `tonic::server::Grpc::unary`/`server_streaming` decode the pb message
//! via `ProstCodec` before this code ever sees it, so the raw bytes aren't
//! available here — only the already-decoded message. This module
//! re-encodes it via `prost::Message::encode_to_vec()` as the signed
//! `CanonicalRequest.body`, which is byte-identical to the wire only when
//! the client's own encoder produces the same field ordering/varint
//! encoding prost's does (true for every prost-based client; not
//! guaranteed for a hand-rolled or non-Rust encoder that legally produces
//! different-but-equivalent protobuf bytes). Closing this gap for real
//! needs a custom `tonic::codec::Decoder` that captures raw bytes
//! alongside the parsed message — not attempted in this pass.

use cratestack_core::{Field, Model, Schema};
use quote::quote;

use crate::shared::{ident, pluralize, to_snake_case};

use crate::include::grpc_pb::fields::model_allows_create;

pub(super) fn build_service(
    schema: &Schema,
    package: &str,
    models_with_pk: &[(&Model, &Field)],
) -> proc_macro2::TokenStream {
    if models_with_pk.is_empty() {
        return quote! {};
    }
    let service_full_name = format!("{package}.Api");
    let mut arms = Vec::new();
    for (model, pk) in models_with_pk {
        arms.push(build_list_arm(package, model));
        arms.push(build_get_arm(package, model, pk));
        if model_allows_create(model) {
            arms.push(build_create_arm(package, model));
        }
        arms.push(build_update_arm(package, model, pk));
        arms.push(build_delete_arm(package, model, pk));
    }
    let _ = schema;

    quote! {
        pub struct ApiServer<C, Auth> {
            state: super::axum::ModelRouterState<C, Auth>,
        }

        impl<C, Auth> ApiServer<C, Auth> {
            pub fn new(state: super::axum::ModelRouterState<C, Auth>) -> Self {
                Self { state }
            }
        }

        impl<C: Clone, Auth: Clone> Clone for ApiServer<C, Auth> {
            fn clone(&self) -> Self {
                Self { state: self.state.clone() }
            }
        }

        impl<C, Auth, B> ::cratestack::grpc::tonic::codegen::Service<::cratestack::grpc::tonic::codegen::http::Request<B>>
            for ApiServer<C, Auth>
        where
            C: ::cratestack::HttpTransport + Send + Sync + 'static,
            Auth: ::cratestack::AuthProvider + Send + Sync + 'static,
            B: ::cratestack::grpc::tonic::codegen::Body + ::core::marker::Send + 'static,
            B::Error: ::core::convert::Into<::cratestack::grpc::tonic::codegen::StdError> + ::core::marker::Send + 'static,
        {
            type Response = ::cratestack::grpc::tonic::codegen::http::Response<::cratestack::grpc::tonic::body::Body>;
            type Error = ::core::convert::Infallible;
            type Future = ::cratestack::grpc::tonic::codegen::BoxFuture<Self::Response, Self::Error>;

            fn poll_ready(
                &mut self,
                _cx: &mut ::cratestack::grpc::tonic::codegen::Context<'_>,
            ) -> ::cratestack::grpc::tonic::codegen::Poll<::core::result::Result<(), Self::Error>> {
                ::cratestack::grpc::tonic::codegen::Poll::Ready(Ok(()))
            }

            fn call(&mut self, req: ::cratestack::grpc::tonic::codegen::http::Request<B>) -> Self::Future {
                let state = self.state.clone();
                match req.uri().path() {
                    #(#arms)*
                    _ => Box::pin(async move {
                        let mut response = ::cratestack::grpc::tonic::codegen::http::Response::new(
                            ::cratestack::grpc::tonic::body::Body::default(),
                        );
                        let headers = response.headers_mut();
                        headers.insert(
                            ::cratestack::grpc::tonic::Status::GRPC_STATUS,
                            (::cratestack::grpc::tonic::Code::Unimplemented as i32).into(),
                        );
                        headers.insert(
                            ::cratestack::grpc::tonic::codegen::http::header::CONTENT_TYPE,
                            ::cratestack::grpc::tonic::metadata::GRPC_CONTENT_TYPE,
                        );
                        Ok(response)
                    }),
                }
            }
        }

        impl<C, Auth> ::cratestack::grpc::tonic::server::NamedService for ApiServer<C, Auth> {
            const NAME: &'static str = #service_full_name;
        }

        /// Mounts this schema's `transport grpc` service as a tonic
        /// `Routes`, converted into an `axum::Router` — `tonic::service::
        /// Routes::into_axum_router` merges cleanly into the same
        /// `axum::Router` the REST/RPC bindings already return (verified
        /// axum/tonic version alignment — see the module doc) — then
        /// layers the gRPC-Web translation + CORS wiring on top
        /// (`::cratestack::grpc::apply_grpc_web`, ticket #172,
        /// `docs/design/protobuf.md` §7.4) so the same router is directly
        /// callable from a browser, not just `grpcurl`/native gRPC
        /// clients. That function — not inline codegen here — owns the
        /// layer composition and is unit-tested on its own
        /// (`cratestack-grpc::web`'s tests assert the exposed-headers set
        /// on a real response), so this call site stays a one-liner.
        pub fn into_router<C, Auth>(state: super::axum::ModelRouterState<C, Auth>) -> ::cratestack::axum::Router
        where
            C: ::cratestack::HttpTransport + Send + Sync + 'static,
            Auth: ::cratestack::AuthProvider + Send + Sync + 'static,
        {
            let router = ::cratestack::grpc::tonic::service::Routes::new(ApiServer::new(state)).into_axum_router();
            ::cratestack::grpc::apply_grpc_web(router)
        }
    }
}

fn method_path(package: &str, model: &str, verb_pascal: &str) -> String {
    format!("/{package}.Api/Model{model}{verb_pascal}")
}

/// Shared prelude every arm needs: auth-relevant headers from gRPC
/// metadata (content-type/accept stripped so codec negotiation falls back
/// to the schema's default codec — see `rpc_inputs.rs`'s module doc and
/// `cratestack_axum::rpc::bridge_grpc_response`'s doc for why that
/// matters), and the canonical request path/body used for both auth and
/// the "known gap" signing note in this file's module doc.
fn request_prelude(path: &str) -> proc_macro2::TokenStream {
    quote! {
        let mut headers = ::cratestack::grpc::metadata_to_headers(request.metadata());
        // The dispatch fn's own request/response codec negotiation reads
        // `Content-Type`/`Accept`, not gRPC's own (`application/grpc+proto`)
        // — pin both to the schema's CBOR wire codec explicitly (required:
        // `validate_transport_request_headers_for` treats a *missing*
        // `Content-Type` as an error on write verbs, it does not default).
        // This is also what `bridge_grpc_response` decodes the dispatch
        // response against, so both directions agree on one content type.
        headers.remove(::cratestack::grpc::tonic::codegen::http::header::ACCEPT);
        headers.insert(
            ::cratestack::grpc::tonic::codegen::http::header::CONTENT_TYPE,
            ::cratestack::grpc::tonic::codegen::http::HeaderValue::from_static("application/cbor"),
        );
        let message = request.into_inner();
        let canonical_body = ::cratestack::grpc::prost::Message::encode_to_vec(&message);
        let canonical = super::axum::CanonicalRequest {
            method: "POST",
            path: #path,
            query: None,
            body: canonical_body.as_ref(),
        };
    }
}

fn status_from_bridge_error(
    code_expr: proc_macro2::TokenStream,
    message_expr: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    quote! {
        ::cratestack::grpc::tonic::Status::new(
            ::cratestack::grpc::cool_error_code_to_tonic_code(&#code_expr),
            #message_expr,
        )
    }
}

fn build_get_arm(package: &str, model: &Model, pk: &Field) -> proc_macro2::TokenStream {
    let path = method_path(package, &model.name, "Get");
    let dispatch_ident = ident(&format!(
        "handle_get_{}_dispatch",
        to_snake_case(&model.name)
    ));
    let request_ty = ident(&format!("{}RpcPkInput", model.name));
    let response_ty = ident(&model.name);
    let svc_ident = ident(&format!("Grpc{}GetSvc", model.name));
    let prelude = request_prelude(&path);
    let status = status_from_bridge_error(quote! { code }, quote! { message });
    let _ = pk;
    quote! {
        #path => {
            struct #svc_ident<C, Auth>(super::axum::ModelRouterState<C, Auth>);
            impl<C, Auth> ::cratestack::grpc::tonic::server::UnaryService<pb::#request_ty> for #svc_ident<C, Auth>
            where
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
                    Box::pin(async move {
                        #prelude
                        let id = message.into_pk().map_err(|error| {
                            ::cratestack::grpc::tonic::Status::new(
                                ::cratestack::grpc::cool_error_code_to_tonic_code(error.code()),
                                error.public_message().into_owned(),
                            )
                        })?;
                        let response = super::axum::#dispatch_ident(state.clone(), canonical, headers.clone(), id, None).await;
                        let domain: super::#response_ty = match ::cratestack::__private::bridge_grpc_response(response, &state.codec, &headers).await {
                            Ok(value) => value,
                            Err((code, message)) => return Err(#status),
                        };
                        Ok(::cratestack::grpc::tonic::Response::new(pb::#response_ty::from(&domain)))
                    })
                }
            }
            let svc = #svc_ident(state);
            let codec = ::cratestack::grpc::tonic::codec::ProstCodec::default();
            let mut grpc = ::cratestack::grpc::tonic::server::Grpc::new(codec);
            Box::pin(async move { Ok(grpc.unary(svc, req).await) })
        }
    }
}

fn build_delete_arm(package: &str, model: &Model, pk: &Field) -> proc_macro2::TokenStream {
    let path = method_path(package, &model.name, "Delete");
    let dispatch_ident = ident(&format!(
        "handle_delete_{}_dispatch",
        to_snake_case(&model.name)
    ));
    let request_ty = ident(&format!("{}RpcPkInput", model.name));
    let response_ty = ident(&model.name);
    let svc_ident = ident(&format!("Grpc{}DeleteSvc", model.name));
    let prelude = request_prelude(&path);
    let status = status_from_bridge_error(quote! { code }, quote! { message });
    let _ = pk;
    quote! {
        #path => {
            struct #svc_ident<C, Auth>(super::axum::ModelRouterState<C, Auth>);
            impl<C, Auth> ::cratestack::grpc::tonic::server::UnaryService<pb::#request_ty> for #svc_ident<C, Auth>
            where
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
                    Box::pin(async move {
                        #prelude
                        let id = message.into_pk().map_err(|error| {
                            ::cratestack::grpc::tonic::Status::new(
                                ::cratestack::grpc::cool_error_code_to_tonic_code(error.code()),
                                error.public_message().into_owned(),
                            )
                        })?;
                        let response = super::axum::#dispatch_ident(state.clone(), canonical, headers.clone(), id).await;
                        let domain: super::#response_ty = match ::cratestack::__private::bridge_grpc_response(response, &state.codec, &headers).await {
                            Ok(value) => value,
                            Err((code, message)) => return Err(#status),
                        };
                        Ok(::cratestack::grpc::tonic::Response::new(pb::#response_ty::from(&domain)))
                    })
                }
            }
            let svc = #svc_ident(state);
            let codec = ::cratestack::grpc::tonic::codec::ProstCodec::default();
            let mut grpc = ::cratestack::grpc::tonic::server::Grpc::new(codec);
            Box::pin(async move { Ok(grpc.unary(svc, req).await) })
        }
    }
}

fn build_create_arm(package: &str, model: &Model) -> proc_macro2::TokenStream {
    let path = method_path(package, &model.name, "Create");
    let dispatch_ident = ident(&format!(
        "handle_create_{}_dispatch",
        pluralize(&to_snake_case(&model.name))
    ));
    let request_ty = ident(&format!("Create{}Input", model.name));
    let response_ty = ident(&model.name);
    let svc_ident = ident(&format!("Grpc{}CreateSvc", model.name));
    let prelude = request_prelude(&path);
    let status = status_from_bridge_error(quote! { code }, quote! { message });
    quote! {
        #path => {
            struct #svc_ident<C, Auth>(super::axum::ModelRouterState<C, Auth>);
            impl<C, Auth> ::cratestack::grpc::tonic::server::UnaryService<pb::#request_ty> for #svc_ident<C, Auth>
            where
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
                    Box::pin(async move {
                        #prelude
                        let domain: ::core::result::Result<super::#request_ty, ::cratestack::CoolError> =
                            super::#request_ty::try_from(message);
                        let domain = match domain {
                            Ok(value) => value,
                            Err(error) => {
                                return Err(::cratestack::grpc::tonic::Status::new(
                                    ::cratestack::grpc::cool_error_code_to_tonic_code(error.code()),
                                    error.public_message().into_owned(),
                                ));
                            }
                        };
                        let body_bytes = match ::cratestack::__private::encode_rpc_value(&state.codec, &headers, &domain).await {
                            Ok(bytes) => ::cratestack::axum::body::Bytes::from(bytes),
                            Err(error) => {
                                return Err(::cratestack::grpc::tonic::Status::new(
                                    ::cratestack::grpc::cool_error_code_to_tonic_code(error.code()),
                                    error.public_message().into_owned(),
                                ));
                            }
                        };
                        let response = super::axum::#dispatch_ident(state.clone(), canonical, headers.clone(), body_bytes).await;
                        let domain: super::#response_ty = match ::cratestack::__private::bridge_grpc_response(response, &state.codec, &headers).await {
                            Ok(value) => value,
                            Err((code, message)) => return Err(#status),
                        };
                        Ok(::cratestack::grpc::tonic::Response::new(pb::#response_ty::from(&domain)))
                    })
                }
            }
            let svc = #svc_ident(state);
            let codec = ::cratestack::grpc::tonic::codec::ProstCodec::default();
            let mut grpc = ::cratestack::grpc::tonic::server::Grpc::new(codec);
            Box::pin(async move { Ok(grpc.unary(svc, req).await) })
        }
    }
}

fn build_update_arm(package: &str, model: &Model, pk: &Field) -> proc_macro2::TokenStream {
    let path = method_path(package, &model.name, "Update");
    let dispatch_ident = ident(&format!(
        "handle_update_{}_dispatch",
        to_snake_case(&model.name)
    ));
    let request_ty = ident(&format!("{}RpcUpdateInput", model.name));
    let response_ty = ident(&model.name);
    let svc_ident = ident(&format!("Grpc{}UpdateSvc", model.name));
    let prelude = request_prelude(&path);
    let status = status_from_bridge_error(quote! { code }, quote! { message });
    let _ = pk;
    quote! {
        #path => {
            struct #svc_ident<C, Auth>(super::axum::ModelRouterState<C, Auth>);
            impl<C, Auth> ::cratestack::grpc::tonic::server::UnaryService<pb::#request_ty> for #svc_ident<C, Auth>
            where
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
                    Box::pin(async move {
                        #prelude
                        let (id, patch) = match message.into_id_and_patch() {
                            Ok(value) => value,
                            Err(error) => {
                                return Err(::cratestack::grpc::tonic::Status::new(
                                    ::cratestack::grpc::cool_error_code_to_tonic_code(error.code()),
                                    error.public_message().into_owned(),
                                ));
                            }
                        };
                        let patch_bytes = match ::cratestack::__private::encode_rpc_value(&state.codec, &headers, &patch).await {
                            Ok(bytes) => ::cratestack::axum::body::Bytes::from(bytes),
                            Err(error) => {
                                return Err(::cratestack::grpc::tonic::Status::new(
                                    ::cratestack::grpc::cool_error_code_to_tonic_code(error.code()),
                                    error.public_message().into_owned(),
                                ));
                            }
                        };
                        let response = super::axum::#dispatch_ident(state.clone(), canonical, headers.clone(), id, patch_bytes).await;
                        let domain: super::#response_ty = match ::cratestack::__private::bridge_grpc_response(response, &state.codec, &headers).await {
                            Ok(value) => value,
                            Err((code, message)) => return Err(#status),
                        };
                        Ok(::cratestack::grpc::tonic::Response::new(pb::#response_ty::from(&domain)))
                    })
                }
            }
            let svc = #svc_ident(state);
            let codec = ::cratestack::grpc::tonic::codec::ProstCodec::default();
            let mut grpc = ::cratestack::grpc::tonic::server::Grpc::new(codec);
            Box::pin(async move { Ok(grpc.unary(svc, req).await) })
        }
    }
}

fn build_list_arm(package: &str, model: &Model) -> proc_macro2::TokenStream {
    let path = method_path(package, &model.name, "List");
    let dispatch_ident = ident(&format!(
        "handle_list_{}_dispatch",
        pluralize(&to_snake_case(&model.name))
    ));
    let request_ty = ident(&format!("{}RpcListInput", model.name));
    let response_ty = ident(&format!("PageOf{}", model.name));
    let svc_ident = ident(&format!("Grpc{}ListSvc", model.name));
    let model_ident = ident(&model.name);
    let prelude = request_prelude(&path);
    let status = status_from_bridge_error(quote! { code }, quote! { message });
    // The wire contract always wraps `list` in `PageOf<Model>` (§4.6's
    // gRPC-specific rule — `cratestack-proto::emit::synth_page`'s module
    // doc), but the *dispatch fn we delegate to* is unchanged from
    // REST/RPC, and its response shape still depends on the model's own
    // `@@paged` attribute: paged models genuinely return `Page<Model>`,
    // unpaged ones return a bare `Vec<Model>`. Getting this wrong is a
    // silent codec decode failure (`CoolError::Codec`), not a type error —
    // there is no compile-time signal, only a wrong assumption about what
    // bytes the codec decodes. Branch on it explicitly rather than
    // guessing one shape.
    let bridge_and_wrap = if crate::shared::is_paged_model(model) {
        quote! {
            let page: ::cratestack::Page<super::#model_ident> = match ::cratestack::__private::bridge_grpc_response(response, &state.codec, &headers).await {
                Ok(value) => value,
                Err((code, message)) => return Err(#status),
            };
            pb::#response_ty::from(&page)
        }
    } else {
        quote! {
            let items: Vec<super::#model_ident> = match ::cratestack::__private::bridge_grpc_response(response, &state.codec, &headers).await {
                Ok(value) => value,
                Err((code, message)) => return Err(#status),
            };
            let page = ::cratestack::Page::new(
                items,
                ::cratestack::PageInfo {
                    limit: None,
                    offset: None,
                    has_next_page: false,
                    has_previous_page: false,
                },
            );
            pb::#response_ty::from(&page)
        }
    };
    quote! {
        #path => {
            struct #svc_ident<C, Auth>(super::axum::ModelRouterState<C, Auth>);
            impl<C, Auth> ::cratestack::grpc::tonic::server::UnaryService<pb::#request_ty> for #svc_ident<C, Auth>
            where
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
                    Box::pin(async move {
                        #prelude
                        let domain_query = message.into_domain();
                        let raw_query = ::cratestack::rpc::synthesize_list_query(&domain_query);
                        let response = super::axum::#dispatch_ident(state.clone(), canonical, headers.clone(), raw_query).await;
                        let wire_value = { #bridge_and_wrap };
                        Ok(::cratestack::grpc::tonic::Response::new(wire_value))
                    })
                }
            }
            let svc = #svc_ident(state);
            let codec = ::cratestack::grpc::tonic::codec::ProstCodec::default();
            let mut grpc = ::cratestack::grpc::tonic::server::Grpc::new(codec);
            Box::pin(async move { Ok(grpc.unary(svc, req).await) })
        }
    }
}
