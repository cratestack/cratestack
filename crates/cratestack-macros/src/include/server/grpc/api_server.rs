//! The `ApiServer<R, C, Auth>` tower-service scaffold that `service::
//! build_service` wraps around the CRUD/procedure match arms it collects
//! from `crud_arms.rs`/`procedure_arms.rs` — split out of `service.rs` to
//! keep it under this repo's 200-LoC file convention. See `service.rs`'s
//! module doc for what this shape mirrors (`tonic-build`'s own output)
//! and why.

use quote::quote;

pub(super) fn build_api_server(
    service_full_name: &str,
    arms: &[proc_macro2::TokenStream],
) -> proc_macro2::TokenStream {
    quote! {
        pub struct ApiServer<R, C, Auth> {
            state: super::axum::ProcedureRouterState<R, C, Auth>,
        }

        impl<R, C, Auth> ApiServer<R, C, Auth> {
            pub fn new(state: super::axum::ProcedureRouterState<R, C, Auth>) -> Self {
                Self { state }
            }
        }

        impl<R: Clone, C: Clone, Auth: Clone> Clone for ApiServer<R, C, Auth> {
            fn clone(&self) -> Self {
                Self { state: self.state.clone() }
            }
        }

        impl<R, C, Auth, B> ::cratestack::grpc::tonic::codegen::Service<::cratestack::grpc::tonic::codegen::http::Request<B>>
            for ApiServer<R, C, Auth>
        where
            R: super::procedures::ProcedureRegistry,
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
                // Same trust boundary as REST/RPC (#415): the gRPC router
                // built by `into_router()` is a *separate* `axum::Router`
                // instance, not covered by protecting `router()` alone, so
                // it must independently pick up whatever `Extension<TrustedProxyConfig>`/
                // `ConnectInfo<SocketAddr>` were applied/wired on THIS
                // router. `ClientIpContext::from_extensions` reads both
                // straight off `req.extensions()` rather than through
                // axum's own extractor machinery, which this raw
                // `http::Request<B>` (tonic's `Service` boundary, not an
                // axum handler) never runs.
                let client_ip_ctx = ::cratestack::ClientIpContext::from_extensions(req.extensions());
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

        impl<R, C, Auth> ::cratestack::grpc::tonic::server::NamedService for ApiServer<R, C, Auth> {
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
        ///
        /// Signature mirrors `axum::router(db, registry, codec,
        /// auth_provider)`'s first four arguments (ticket #208 —
        /// previously this took an already-built `ModelRouterState`,
        /// which had no room for a procedure registry; every other
        /// entrypoint in this schema already takes these four arguments
        /// separately, so this one now does too rather than growing a
        /// second, gRPC-only calling convention). Deliberately does
        /// *not* also take `router()`'s `body_limit_bytes` (cratestack#413)
        /// — gRPC framing has its own message-size ceiling (tonic's
        /// default 4 MiB `max_decoding_message_size`), a transport
        /// concern orthogonal to `axum::extract::DefaultBodyLimit`, which
        /// only ever governs the REST/RPC `Bytes` extractors.
        pub fn into_router<R, C, Auth>(
            db: super::Cratestack,
            registry: R,
            codec: C,
            auth_provider: Auth,
        ) -> ::cratestack::axum::Router
        where
            R: super::procedures::ProcedureRegistry,
            C: ::cratestack::HttpTransport + Send + Sync + 'static,
            Auth: ::cratestack::AuthProvider + Send + Sync + 'static,
        {
            let state = super::axum::ProcedureRouterState { db, registry, codec, auth_provider };
            let router = ::cratestack::grpc::tonic::service::Routes::new(ApiServer::new(state)).into_axum_router();
            ::cratestack::grpc::apply_grpc_web(router)
        }
    }
}
