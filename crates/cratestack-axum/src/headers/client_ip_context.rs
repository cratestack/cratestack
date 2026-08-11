use std::convert::Infallible;
use std::net::SocketAddr;

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;

use crate::trusted_proxy::TrustedProxyConfig;

/// The trusted-proxy configuration (if an `Extension<TrustedProxyConfig>`
/// was applied to the router) and the verified socket peer (if the router
/// is served via `into_make_service_with_connect_info`), bundled into a
/// single axum extractor so every generated dispatch fn threads one new
/// parameter instead of two (#415).
///
/// A hand-written `FromRequestParts` impl rather than `Option<Extension<T>>`/
/// `Option<ConnectInfo<T>>` extractor parameters: axum 0.8 only extends its
/// blanket `Option<T>: FromRequestParts` impl to types implementing the
/// separate `OptionalFromRequestParts` trait, which neither `Extension<T>`
/// nor `ConnectInfo<T>` implements — so those two, wrapped in `Option`,
/// are not valid extractor parameter types on this axum version. Reading
/// `Parts::extensions` directly (via the infallible `Extensions` extractor
/// axum-core itself provides) sidesteps that entirely and never fails.
#[derive(Clone, Debug, Default)]
pub struct ClientIpContext {
    pub trusted_proxy: Option<TrustedProxyConfig>,
    pub peer: Option<SocketAddr>,
}

impl ClientIpContext {
    /// Build directly from a raw `http::Extensions` map — the seam the
    /// gRPC transport (`into_router()`, a tonic `Service` rather than an
    /// axum handler) uses, since it never runs axum's own extractor
    /// machinery. REST/RPC handlers get this for free via the
    /// `FromRequestParts` impl below instead.
    pub fn from_extensions(extensions: &http::Extensions) -> Self {
        Self {
            trusted_proxy: extensions.get::<TrustedProxyConfig>().cloned(),
            peer: extensions
                .get::<ConnectInfo<SocketAddr>>()
                .map(|ConnectInfo(addr)| *addr),
        }
    }
}

impl<S> FromRequestParts<S> for ClientIpContext
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self::from_extensions(&parts.extensions))
    }
}
