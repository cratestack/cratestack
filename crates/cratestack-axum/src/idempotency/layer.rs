//! Tower layer + companion `Service` constructor.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::Request;
use cratestack_core::CratestackError;
use cratestack_exec::{OpAdmission, OpExecutor};
use tower::Layer;

pub(super) use super::fingerprint::default_principal_fingerprint;
use super::fingerprint::legacy_principal_fingerprint;
use super::service::IdempotencyService;
use super::store::IdempotencyStore;

/// Tower layer that wires an `IdempotencyStore` into the request pipeline.
///
/// Since ADR 0015 slice 1 the decision itself lives at L3 in
/// [`cratestack_exec::OpExecutor`]; this layer is the HTTP adapter around
/// it, owning exactly the things L3 may not name — the `Idempotency-Key`
/// header, the request fingerprint, the principal derivation, and the
/// response shapes.
#[derive(Clone)]
pub struct IdempotencyLayer {
    pub(super) executor: OpExecutor,
    pub(super) principal_fingerprint:
        Arc<dyn Fn(&Request) -> Result<String, CratestackError> + Send + Sync>,
    pub(super) op_resolver: Arc<dyn Fn(&Request) -> OpAdmission + Send + Sync>,
}

impl IdempotencyLayer {
    /// Construct with a default principal fingerprint derived from a
    /// `VerifiedPrincipal` request extension when an upstream layer (the
    /// COSE envelope layer, cratestack#1006) inserted one, otherwise from the
    /// `Authorization` header, falling back to the verified TCP peer address
    /// (via axum's `ConnectInfo<SocketAddr>`, requires serving through
    /// `into_make_service_with_connect_info::<SocketAddr>()`) when it's
    /// absent. If *neither* is available the request is refused rather than
    /// silently placed in a shared `"anonymous"` namespace (cratestack#416)
    /// — callers running mTLS or session-cookie auth, or who cannot serve
    /// through `into_make_service_with_connect_info`, must supply
    /// [`with_principal_fingerprint`](Self::with_principal_fingerprint)
    /// explicitly.
    pub fn new(store: Arc<dyn IdempotencyStore>, ttl: Duration) -> Self {
        Self {
            executor: OpExecutor::new(Some(store), ttl),
            principal_fingerprint: Arc::new(default_principal_fingerprint),
            op_resolver: Arc::new(|_| OpAdmission::unresolved()),
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

    /// Keep the default fingerprint as it was before cratestack#1006: the
    /// `Authorization` header, then the `ConnectInfo` peer, then a `412`,
    /// ignoring any `VerifiedPrincipal`. See
    /// [`legacy_principal_fingerprint`](super::legacy_principal_fingerprint)
    /// for when a deployment wants this across an upgrade.
    pub fn with_legacy_principal_fingerprint(mut self) -> Self {
        self.principal_fingerprint = Arc::new(legacy_principal_fingerprint);
        self
    }

    /// Teach the layer which schema op each request is about, so
    /// `@no_idempotency` (and every read) can skip reservation.
    ///
    /// Mirrors [`crate::ratelimit::RateLimitLayer::with_op_resolver`], which
    /// accepts the same resolvers
    /// — pass [`build_rest_op_resolver`] over the generated
    /// `ROUTE_TRANSPORTS`, or [`build_rpc_op_resolver`] over `OPS`.
    ///
    /// **Not installing one is a supported configuration and changes
    /// nothing.** The default resolver reports every request as
    /// [`OpAdmission::unresolved`], which reserves — so an existing
    /// consumer that never calls this method reserves exactly the set of
    /// requests it always did. That is the property ADR 0015 slice 1's
    /// byte-identity bar rests on, and it is why this is opt-in rather
    /// than wired automatically.
    ///
    /// [`build_rest_op_resolver`]: super::build_rest_op_resolver
    /// [`build_rpc_op_resolver`]: super::build_rpc_op_resolver
    pub fn with_op_resolver(
        mut self,
        f: impl Fn(&Request) -> OpAdmission + Send + Sync + 'static,
    ) -> Self {
        self.op_resolver = Arc::new(f);
        self
    }
}

impl<S> Layer<S> for IdempotencyLayer {
    type Service = IdempotencyService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        IdempotencyService {
            inner,
            executor: self.executor.clone(),
            principal_fingerprint: self.principal_fingerprint.clone(),
            op_resolver: self.op_resolver.clone(),
        }
    }
}
