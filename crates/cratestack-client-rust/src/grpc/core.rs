//! [`CratestackGrpcClient`] — gRPC sibling of `crate::client::
//! CratestackClient` (REST) and `crate::rpc::RpcClient` (RPC). Wraps a
//! `tonic::client::Grpc<T>`, carries the same `RequestAuthorizer` /
//! schema-sha conventions those two already use, and exposes one `unary`
//! helper every generated per-model gRPC client method
//! (`cratestack-macros::include::client::grpc::tonic_client`) calls into —
//! the client-side twin of the server's hand-rolled `tonic::server::Grpc`
//! service arms.

use std::sync::Arc;

use tonic::client::{Grpc, GrpcService};
use tonic::codec::ProstCodec;
use tonic::codegen::{Body, Bytes, StdError};
use tonic::metadata::{Ascii, MetadataValue};

use crate::auth::{AuthorizationRequest, RequestAuthorizer};

use super::canonical::{GRPC_CONTENT_TYPE, grpc_canonical_request_string, grpc_method_path};
use super::error::GrpcClientError;

/// Same header name `cratestack-axum::schema_fingerprint::SCHEMA_SHA_HEADER`
/// checks server-side (issue #178) and
/// `crate::client::headers::SCHEMA_SHA_HEADER` sends over REST/RPC —
/// duplicated rather than shared, same rationale as that constant's own
/// comment (this crate has no reason to depend on the server-runtime
/// crate for one string).
const SCHEMA_SHA_HEADER: &str = "x-cratestack-schema-sha";

/// `Debug` is implemented manually below rather than derived:
/// `request_authorizer` is `Option<Arc<dyn RequestAuthorizer>>`, and
/// `RequestAuthorizer` has no `Debug` supertrait — `#[derive(Debug)]`
/// would fail to compile on this field regardless of `T`. The
/// macro-generated `Client<T>` (`cratestack-macros::include::client::
/// grpc::tonic_client`) derives `Debug`, so this type must provide one.
#[derive(Clone)]
pub struct CratestackGrpcClient<T> {
    inner: Grpc<T>,
    /// The schema's locked `.pb.lock` package name — baked in once here
    /// so every per-model generated method only has to pass its bare
    /// method name (`"ModelWidgetGet"`), not the full `/pkg.Api/Method`
    /// path, to [`Self::unary`].
    package: &'static str,
    request_authorizer: Option<Arc<dyn RequestAuthorizer>>,
    schema_sha: Option<&'static str>,
}

impl<T> CratestackGrpcClient<T> {
    /// Wrap any `T` a `tonic::client::Grpc<T>` accepts — a
    /// `tonic::transport::Channel` (the common case), a test double, or an
    /// interceptor-wrapped service. `package` is stamped on by the
    /// generated `Client::new`, not meant to be passed by schema authors
    /// directly.
    pub fn new(inner: T, package: &'static str) -> Self {
        Self {
            inner: Grpc::new(inner),
            package,
            request_authorizer: None,
            schema_sha: None,
        }
    }

    /// Attach a `RequestAuthorizer` — the same envelope-signing convention
    /// `CratestackClient::with_request_authorizer` (REST/RPC) uses. Every
    /// subsequent `unary` call asks it for headers, computed over the
    /// canonical request string built from this call's **unframed**
    /// prost-encoded bytes (`crate::grpc::canonical` — see that module's
    /// doc for why no frame-stripping is needed on the client side).
    pub fn with_request_authorizer(
        mut self,
        request_authorizer: Arc<dyn RequestAuthorizer>,
    ) -> Self {
        self.request_authorizer = Some(request_authorizer);
        self
    }

    /// Stamps the generating schema's `SCHEMA_SHA256` onto this client
    /// (issue #178) — same warn-only drift-detection header REST/RPC
    /// already sends, carried as gRPC metadata here instead of an HTTP
    /// header (gRPC metadata *is* HTTP/2 headers on the wire —
    /// `cratestack_grpc::metadata`'s own module doc). Called by the
    /// schema-generated `Client::new`, not meant to be called directly by
    /// schema authors.
    pub fn with_schema_sha(mut self, schema_sha: &'static str) -> Self {
        self.schema_sha = Some(schema_sha);
        self
    }

    /// One unary gRPC call: encodes `message` via prost, computes the
    /// envelope-signing canonical string over the resulting **unframed**
    /// bytes, asks the configured `RequestAuthorizer` (if any) for
    /// headers, attaches them plus the schema-sha header as gRPC metadata,
    /// and dispatches via `tonic::client::Grpc::unary` — mirroring what
    /// `tonic-build`'s own generated client methods do inline, factored
    /// into one place so every generated CRUD method is a few lines
    /// instead of duplicating this plumbing per verb (see this module's
    /// doc).
    pub async fn unary<Req, Resp>(
        &mut self,
        method_name: &str,
        message: Req,
    ) -> Result<Resp, GrpcClientError>
    where
        T: GrpcService<tonic::body::Body>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + Send,
        Req: prost::Message + Clone + Default + 'static,
        Resp: prost::Message + Default + 'static,
    {
        self.inner
            .ready()
            .await
            .map_err(|error| GrpcClientError::Transport(error.into()))?;

        let path = tonic::codegen::http::uri::PathAndQuery::try_from(grpc_method_path(
            self.package,
            method_name,
        ))
        .map_err(|error| GrpcClientError::BadInput(error.to_string()))?;

        let mut request = tonic::Request::new(message.clone());

        if let Some(authorizer) = &self.request_authorizer {
            let body = prost::Message::encode_to_vec(&message);
            let canonical_request = grpc_canonical_request_string(self.package, method_name, &body);
            let authorization_request = AuthorizationRequest {
                method: "POST".to_owned(),
                path: grpc_method_path(self.package, method_name),
                canonical_query: None,
                content_type: Some(GRPC_CONTENT_TYPE.to_owned()),
                body,
                canonical_request,
            };
            let headers = authorizer
                .authorize(&authorization_request)
                .await
                .map_err(|error| GrpcClientError::BadInput(error.to_string()))?;
            let metadata = request.metadata_mut();
            for (name, value) in headers {
                let key = tonic::metadata::MetadataKey::<Ascii>::from_bytes(name.as_bytes())
                    .map_err(|error| {
                        GrpcClientError::BadInput(format!("invalid header name '{name}': {error}"))
                    })?;
                let value = MetadataValue::try_from(value.as_str()).map_err(|error| {
                    GrpcClientError::BadInput(format!("invalid header value for '{name}': {error}"))
                })?;
                metadata.insert(key, value);
            }
        }

        if let Some(schema_sha) = self.schema_sha {
            let value = MetadataValue::try_from(schema_sha)
                .map_err(|error| GrpcClientError::BadInput(error.to_string()))?;
            request.metadata_mut().insert(SCHEMA_SHA_HEADER, value);
        }

        let codec = ProstCodec::default();
        let response = self.inner.unary(request, path, codec).await?;
        Ok(response.into_inner())
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for CratestackGrpcClient<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CratestackGrpcClient")
            .field("inner", &self.inner)
            .field("package", &self.package)
            .field("request_authorizer", &self.request_authorizer.is_some())
            .field("schema_sha", &self.schema_sha)
            .finish()
    }
}
