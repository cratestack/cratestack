//! `Client<T>` — the outer gRPC client struct, `connect`/`new`/
//! `with_request_authorizer`, and per-model accessor methods
//! (`client.widgets()`). Split from [`super::model_api`] (which builds
//! each `<Model>GrpcApi<T>`'s CRUD methods) to stay under the repo's
//! 200-LoC file convention — see that module's doc for the shared design
//! rationale (mirrors `tonic-build`'s own generated client shape, factors
//! the repetitive plumbing into `CratestackGrpcClient::unary`).

use cratestack_core::{Field, Model};
use quote::quote;

use crate::shared::{ident, pluralize, to_snake_case};

/// `Client<T>` itself, plus accessor methods for every model with a
/// primary key. `package`: the schema's locked `.pb.lock` package name,
/// baked in once at `Client::new` so every per-model method only needs to
/// pass its bare method name to `CratestackGrpcClient::unary`.
pub(super) fn build_client_struct(
    package: &str,
    models_with_pk: &[(&Model, &Field)],
) -> proc_macro2::TokenStream {
    let model_accessors = models_with_pk
        .iter()
        .map(|(model, _pk)| {
            let accessor_ident = ident(&pluralize(&to_snake_case(&model.name)));
            let api_ident = ident(&format!("{}GrpcApi", model.name));
            quote! {
                pub fn #accessor_ident(&self) -> #api_ident<T> {
                    #api_ident { grpc: self.grpc.clone() }
                }
            }
        })
        .collect::<Vec<_>>();

    quote! {
        /// Native `tonic`-based gRPC client (ticket #209) — the
        /// `include_client_schema!` twin of `include_server_schema!`'s
        /// generated tonic service (ticket #171). Treats the schema
        /// purely as a contract: no DB, no router, no policy enforcement
        /// of its own (the server enforces policy; this client just calls
        /// it).
        #[derive(Debug, Clone)]
        pub struct Client<T = ::cratestack::grpc::tonic::transport::Channel> {
            grpc: ::cratestack::client_rust::grpc::CratestackGrpcClient<T>,
        }

        impl Client<::cratestack::grpc::tonic::transport::Channel> {
            /// Attempt to create a new client by connecting to a given
            /// endpoint. Mirrors `tonic-build`'s own generated
            /// `XClient::connect`.
            pub async fn connect<D>(
                dst: D,
            ) -> ::core::result::Result<Self, ::cratestack::grpc::tonic::transport::Error>
            where
                D: ::core::convert::TryInto<::cratestack::grpc::tonic::transport::Endpoint>,
                D::Error: ::core::convert::Into<::cratestack::grpc::tonic::codegen::StdError>,
            {
                let conn = ::cratestack::grpc::tonic::transport::Endpoint::new(dst)?
                    .connect()
                    .await?;
                Ok(Self::new(conn))
            }
        }

        impl<T> Client<T> {
            /// Build a client wrapping any `T: GrpcService` — a
            /// `tonic::transport::Channel` (the common case, or use
            /// `connect` above), a test double, or an
            /// interceptor-wrapped service.
            pub fn new(inner: T) -> Self {
                Self {
                    grpc: ::cratestack::client_rust::grpc::CratestackGrpcClient::new(inner, #package)
                        .with_schema_sha(super::SCHEMA_SHA256),
                }
            }

            /// Attach a `RequestAuthorizer` — the same envelope-signing
            /// convention `CratestackClient::with_request_authorizer`
            /// (REST/RPC) uses, so a schema author configures auth once
            /// regardless of transport. See `cratestack-client-rust`'s
            /// `grpc::canonical` module for how the signed bytes are
            /// derived for a gRPC call specifically.
            pub fn with_request_authorizer(
                mut self,
                request_authorizer: ::std::sync::Arc<dyn ::cratestack::client_rust::RequestAuthorizer>,
            ) -> Self {
                self.grpc = self.grpc.with_request_authorizer(request_authorizer);
                self
            }
        }

        impl<T> Client<T>
        where
            T: ::core::clone::Clone,
        {
            #(#model_accessors)*
        }
    }
}
