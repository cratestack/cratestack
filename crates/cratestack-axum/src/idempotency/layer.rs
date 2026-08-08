//! Tower layer + companion `Service` constructor.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::Request;
use http::header;
use sha2::{Digest, Sha256};
use tower::Layer;

use super::service::IdempotencyService;
use super::store::IdempotencyStore;
use crate::headers::parse_client_ip;

/// Tower layer that wires an `IdempotencyStore` into the request pipeline.
#[derive(Clone)]
pub struct IdempotencyLayer {
    pub(super) store: Arc<dyn IdempotencyStore>,
    pub(super) ttl: Duration,
    pub(super) principal_fingerprint: Arc<dyn Fn(&Request) -> String + Send + Sync>,
}

impl IdempotencyLayer {
    /// Construct with a default principal fingerprint derived from the
    /// `Authorization` header. Callers running mTLS or session-cookie auth
    /// should swap this via [`with_principal_fingerprint`].
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

    // Fall back to client IP for unauthenticated requests to avoid collisions
    // between distinct callers.
    if let Some(client_ip) = parse_client_ip(req.headers()) {
        return client_ip;
    }

    // Only if both Authorization and client IP are absent, use "anonymous".
    // This should rarely occur in practice; most requests have at least a
    // client IP (via X-Forwarded-For or Forwarded header in proxied scenarios).
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
