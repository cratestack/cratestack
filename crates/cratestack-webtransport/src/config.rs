//! Server configuration. Tuning numbers live here, never in `.cstack`
//! (extensions.md: the schema declares participation, not numbers).

use std::net::SocketAddr;
use std::time::Duration;

use cratestack_core::{CratestackContext, DEFAULT_BODY_LIMIT_BYTES};

use crate::origin::OriginPolicy;
use crate::principal::default_principal_key;
use crate::transport::{CertificateDer, PrivateKeyDer};

/// Per-session and per-principal bounds (design §6.6, §6a.4).
#[derive(Debug, Clone)]
pub struct WebTransportLimits {
    /// Concurrent subscriptions on one session. Past it, a new subscription
    /// gets `resource_exhausted` and the session stays up.
    pub subscriptions_per_session: usize,
    /// Concurrent sessions per principal (anonymous callers are counted per
    /// remote IP). Past it, the session closes with 4429.
    pub sessions_per_principal: usize,
    /// Largest frame accepted from a client, in bytes.
    pub max_frame_bytes: usize,
    /// How long a new session may take to send `Init`.
    pub init_timeout: Duration,
    /// QUIC keepalive interval.
    pub keep_alive: Duration,
    /// QUIC idle timeout; must exceed `keep_alive`.
    pub idle_timeout: Duration,
}

impl Default for WebTransportLimits {
    fn default() -> Self {
        Self {
            subscriptions_per_session: 64,
            sessions_per_principal: 8,
            max_frame_bytes: DEFAULT_BODY_LIMIT_BYTES,
            init_timeout: Duration::from_secs(10),
            keep_alive: Duration::from_secs(15),
            idle_timeout: Duration::from_secs(30),
        }
    }
}

/// Derives the identity a session is counted and re-authenticated under.
/// `None` means anonymous.
pub type PrincipalKeyFn = fn(&CratestackContext) -> Option<String>;

/// Everything the QUIC listener needs.
pub struct WebTransportConfig {
    pub(crate) bind: SocketAddr,
    pub(crate) path: String,
    pub(crate) certificate_chain: Vec<CertificateDer<'static>>,
    pub(crate) private_key: PrivateKeyDer<'static>,
    pub(crate) origins: OriginPolicy,
    pub(crate) limits: WebTransportLimits,
    pub(crate) principal_key: PrincipalKeyFn,
}

impl WebTransportConfig {
    /// A listener on `bind` serving `certificate_chain`. Defaults: path
    /// `/rpc/wt`, same-origin browsers only, [`WebTransportLimits::default`].
    pub fn new(
        bind: SocketAddr,
        certificate_chain: Vec<CertificateDer<'static>>,
        private_key: PrivateKeyDer<'static>,
    ) -> Self {
        Self {
            bind,
            path: "/rpc/wt".to_owned(),
            certificate_chain,
            private_key,
            origins: OriginPolicy::SameOrigin,
            limits: WebTransportLimits::default(),
            principal_key: default_principal_key,
        }
    }

    /// The session path: `/rpc/wt` for `transport rpc` schemas, `/$wt` for
    /// REST schemas (design §7.2). Any other path is refused with 404.
    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }

    pub fn origins(mut self, origins: OriginPolicy) -> Self {
        self.origins = origins;
        self
    }

    pub fn limits(mut self, limits: WebTransportLimits) -> Self {
        self.limits = limits;
        self
    }

    pub fn principal_key(mut self, principal_key: PrincipalKeyFn) -> Self {
        self.principal_key = principal_key;
        self
    }
}
