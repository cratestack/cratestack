//! Tower layer + companion `Service` constructor.

use std::net::SocketAddr;
use std::sync::{Arc, Once};
use std::time::Duration;

use axum::extract::{ConnectInfo, Request};
use cratestack_core::CoolError;
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
    pub(super) principal_fingerprint:
        Arc<dyn Fn(&Request) -> Result<String, CoolError> + Send + Sync>,
}

impl IdempotencyLayer {
    /// Construct with a default principal fingerprint derived from the
    /// `Authorization` header, falling back to the verified TCP peer address
    /// (via axum's `ConnectInfo<SocketAddr>`, requires serving through
    /// `into_make_service_with_connect_info::<SocketAddr>()`) when it's
    /// absent. If *neither* is available the request is refused rather than
    /// silently placed in a shared `"anonymous"` namespace (cratestack#416)
    /// — callers running mTLS or session-cookie auth, or who cannot serve
    /// through `into_make_service_with_connect_info`, must supply
    /// [`with_principal_fingerprint`] explicitly.
    pub fn new(store: Arc<dyn IdempotencyStore>, ttl: Duration) -> Self {
        Self {
            store,
            ttl,
            principal_fingerprint: Arc::new(default_principal_fingerprint),
        }
    }

    /// Override how the layer derives a principal-scoped namespace for the
    /// idempotency key. Without this, two callers sharing a key (across
    /// tenants) would collide. The supplied closure is infallible by design
    /// — a caller who opts out of the default's fail-closed behavior is
    /// taking explicit responsibility for the namespace it returns,
    /// including any deliberate shared bucket.
    pub fn with_principal_fingerprint(
        mut self,
        f: impl Fn(&Request) -> String + Send + Sync + 'static,
    ) -> Self {
        self.principal_fingerprint = Arc::new(move |req| Ok(f(req)));
        self
    }
}

/// Logged once per process, not per request — a busy misconfigured
/// deployment would otherwise emit this thousands of times a second. See
/// `default_principal_fingerprint` for the condition that fires it.
static MISSING_IDENTITY_WARNING: Once = Once::new();

/// cratestack#416: the pre-existing default silently collapsed every
/// unauthenticated caller without a verified peer address onto a single
/// shared `"anonymous"` idempotency namespace — two distinct callers reusing
/// an `Idempotency-Key` could then replay each other's response. Refusing
/// the request instead (`PreconditionFailed`, matching this crate's
/// established "handled error, not an unwind" shape) makes the gap loud in
/// staging/CI instead of silently reachable in production, per the
/// ticket's Expected Behavior: "construction requires an explicit
/// fingerprint function so the collision cannot be reached by accident."
pub(super) fn default_principal_fingerprint(req: &Request) -> Result<String, CoolError> {
    // Prefer Authorization header for authenticated requests.
    if let Some(auth_header) = req.headers().get(header::AUTHORIZATION)
        && let Ok(auth_str) = auth_header.to_str()
    {
        let mut h = Sha256::new();
        h.update(auth_str.as_bytes());
        return Ok(format!("{:x}", h.finalize()));
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
        return Ok(addr.ip().to_string());
    }

    // Neither Authorization nor a verified peer address is available (e.g.
    // the server isn't wired through `into_make_service_with_connect_info`).
    // There is no unforgeable value left to key on, so refuse rather than
    // collapsing every such caller onto one shared namespace.
    MISSING_IDENTITY_WARNING.call_once(|| {
        tracing::warn!(
            target: "cratestack",
            cratestack_operation = "idempotency",
            "IdempotencyLayer's default principal fingerprint has no Authorization header and \
             no ConnectInfo<SocketAddr> peer on this request, so it cannot verify caller \
             identity. Refusing the request rather than collapsing distinct callers onto a \
             shared \"anonymous\" namespace (cratestack#416) — wire \
             into_make_service_with_connect_info::<SocketAddr>() or supply \
             IdempotencyLayer::with_principal_fingerprint(...) explicitly. Logged once per \
             process; every matching request is refused until this is fixed.",
        );
    });
    Err(CoolError::PreconditionFailed(
        "idempotency: no verifiable caller identity (Authorization header or ConnectInfo peer) \
         is available for the default namespace fingerprint; the server must be served through \
         into_make_service_with_connect_info::<SocketAddr>() or configure an explicit \
         fingerprint function"
            .to_owned(),
    ))
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
