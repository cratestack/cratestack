//! [`StreamableHttpService`]: the tower service an application mounts, and
//! the guard in front of `rmcp`'s Streamable HTTP service.
//!
//! In this order, each step before any later one runs:
//!
//! ```text
//! Origin not allowed                 -> 403   (before any MCP handling)
//! not POST                           -> 405
//! `access_token` in the query        -> 400 + WWW-Authenticate (strict.rs)
//! no bearer token                    -> 401 + WWW-Authenticate
//! body over 4 MiB                    -> 413
//! AuthProvider refuses / no identity -> 401 (403, 5xx) + WWW-Authenticate
//! a mirrored MCP header sent twice   -> 400 / -32020 (strict.rs)
//! Authorization removed, caller attached to the request, then rmcp:
//!   Host, Origin again, MCP-Protocol-Version / Mcp-Method / Mcp-Name
//!   against the body (400 / -32020), then the handler
//! ```
//!
//! Authenticating in a layer rather than in the handler keeps 401 and 403
//! out of JSON-RPC, so an unauthenticated request never reaches MCP parsing
//! (cratestack#1039's implementation note).
//!
//! **The token is not forwarded.** The `Authorization` header is removed
//! before `rmcp` sees the request, so neither `rmcp` nor a tool handler nor
//! anything that logs a request downstream ever holds it. The provider saw
//! it and turned it into a `CratestackContext`, and that context is all
//! the call runs with.

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use cratestack_core::AuthProvider;
use http::{Method, Request, StatusCode};
use http_body::Body;
use http_body_util::{BodyExt, Full, LengthLimitError, Limited};
use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;

use super::auth::{Bearer, authenticate, bearer, missing_token};
use super::caller::hand_over;
use super::origin::AllowedOrigins;
use super::reply::{self, Reply};
use super::resource::Resolved;
use super::strict;
use crate::server::McpServer;
use crate::table::McpTools;

/// The request body limit, `rmcp`'s own default. The guard reads the body
/// before the provider runs, so it has to bound it itself.
pub(crate) const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

pub(crate) type Inner<T> =
    rmcp::transport::StreamableHttpService<Arc<McpServer<T>>, NeverSessionManager>;

pub(crate) struct Shared<T: McpTools, A> {
    pub(crate) inner: Inner<T>,
    pub(crate) provider: A,
    pub(crate) origins: AllowedOrigins,
    pub(crate) resource: Resolved,
}

/// The MCP endpoint, as a tower service: mount it with
/// `Router::nest_service("/mcp", ...)` (or any path that matches the
/// resource identifier). Cheap to clone.
pub struct StreamableHttpService<T: McpTools, A> {
    pub(crate) shared: Arc<Shared<T, A>>,
}

impl<T: McpTools, A> Clone for StreamableHttpService<T, A> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<T: McpTools, A: AuthProvider> Shared<T, A> {
    async fn handle<B>(&self, request: Request<B>) -> Reply
    where
        B: Body + Send + 'static,
        B::Data: Send,
        B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        if !self.origins.permits(request.headers()) {
            tracing::warn!(
                target: "cratestack",
                cratestack_operation = "mcp_http_origin",
                origin = ?request.headers().get(http::header::ORIGIN),
                "mcp: request refused: Origin not allowed",
            );
            return reply::forbidden_origin();
        }
        if request.method() != Method::POST {
            return reply::method_not_allowed();
        }
        if strict::query_token(request.uri()) {
            let header = self.resource.challenge(Some("invalid_request"));
            return reply::challenge(StatusCode::BAD_REQUEST, header);
        }
        let token = match bearer(request.headers()) {
            Bearer::Token(token) => token.to_owned(),
            Bearer::Missing => return missing_token(&self.resource),
            Bearer::Malformed => {
                let header = self.resource.challenge(Some("invalid_request"));
                return reply::challenge(StatusCode::BAD_REQUEST, header);
            }
        };

        let (mut parts, body) = request.into_parts();
        let body = match Limited::new(body, MAX_BODY_BYTES).collect().await {
            Ok(collected) => collected.to_bytes(),
            Err(error) if error.downcast_ref::<LengthLimitError>().is_some() => {
                return reply::payload_too_large();
            }
            Err(_) => return reply::bad_body(),
        };
        let caller = match authenticate(&self.provider, &self.resource, &parts, &body, &token).await
        {
            Ok(caller) => caller,
            Err(refusal) => return *refusal,
        };
        if let Some(name) = strict::repeated_mirror(&parts.headers) {
            return reply::header_mismatch(format!("the {name} header appears more than once"));
        }

        hand_over(&mut parts, caller);
        self.inner
            .handle(Request::from_parts(parts, Full::new(body)))
            .await
    }
}

impl<T, A, B> tower::Service<Request<B>> for StreamableHttpService<T, A>
where
    T: McpTools,
    A: AuthProvider,
    B: Body + Send + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    type Response = Reply;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Reply, Infallible>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<B>) -> Self::Future {
        let shared = Arc::clone(&self.shared);
        Box::pin(async move { Ok(shared.handle(request).await) })
    }
}
