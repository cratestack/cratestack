//! Tower layer + companion `Service` constructor.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{ConnectInfo, Request};
use http::header;
use sha2::{Digest, Sha256};
use tower::Layer;

use super::service::IdempotencyService;
use super::store::IdempotencyStore;

/// Tower layer that wires an `IdempotencyStore` into the request pipeline.
#[derive(Clone)]
pub struct IdempotencyLayer {
    pub(super) store: Arc<dyn IdempotencyStore>,
    pub(super) ttl: Duration,
    pub(super) principal_fingerprint: Arc<dyn Fn(&Request) -> String + Send + Sync>,
}

impl IdempotencyLayer {
    /// Construct with a default principal fingerprint derived from the
    /// `Authorization` header, falling back to the verified TCP peer address
    /// (via axum's `ConnectInfo<SocketAddr>`, requires serving through
    /// `into_make_service_with_connect_info::<SocketAddr>()`) when it's
    /// absent. Callers running mTLS or session-cookie auth should swap this
    /// via [`with_principal_fingerprint`].
    pub fn new(store: Arc<dyn IdempotencyStore>, ttl: Duration) -> Self {
        Self {
            store,
            ttl,
            principal_fingerprint: Arc::new(default_principal_fingerprint),
        }
    }

    /// Override how the layer derives a principal-scoped namespace for the
    /// idempotency key. Without this, two callers sharing a key (across
    /// tenants) would collide.
    pub fn with_principal_fingerprint(
        mut self,
        f: impl Fn(&Request) -> String + Send + Sync + 'static,
    ) -> Self {
        self.principal_fingerprint = Arc::new(f);
        self
    }
}

pub(super) fn default_principal_fingerprint(req: &Request) -> String {
    // Prefer Authorization header for authenticated requests.
    if let Some(auth_header) = req.headers().get(header::AUTHORIZATION)
        && let Ok(auth_str) = auth_header.to_str()
    {
        let mut h = Sha256::new();
        h.update(auth_str.as_bytes());
        return format!("{:x}", h.finalize());
    }

    // Fall back to the real TCP peer address for unauthenticated requests, to
    // avoid collisions between distinct callers. This is deliberately *not*
    // `Forwarded`/`X-Forwarded-For`: those headers are client-supplied and
    // this crate has no trusted-proxy configuration to verify or strip them,
    // so trusting them here would let an attacker land in another caller's
    // idempotency namespace just by guessing/spoofing that caller's apparent
    // IP. `ConnectInfo` is populated by axum from the actual accepted socket
    // (when the server is served via `into_make_service_with_connect_info::<SocketAddr>()`)
    // and cannot be spoofed by the client.
    if let Some(ConnectInfo(addr)) = req.extensions().get::<ConnectInfo<SocketAddr>>() {
        return addr.ip().to_string();
    }

    // Only if both Authorization and a verified peer address are absent
    // (e.g. the server isn't wired through `into_make_service_with_connect_info`),
    // fall back to a single shared namespace. This matches the pre-existing,
    // safe-by-default behavior: unauthenticated traffic that can't be
    // distinguished falls back to the coarse default rather than trusting an
    // unverifiable, attacker-controlled key.
    "anonymous".to_owned()
}

impl<S> Layer<S> for IdempotencyLayer {
    type Service = IdempotencyService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        IdempotencyService {
            inner,
            store: self.store.clone(),
            ttl: self.ttl,
            principal_fingerprint: self.principal_fingerprint.clone(),
        }
    }
}
