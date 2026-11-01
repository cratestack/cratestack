//! Server configuration. Tuning numbers live here, never in `.cstack`
//! (extensions.md: the schema declares participation, not numbers).

use std::time::Duration;

use cratestack_core::subscription_wire::{OriginPolicy, default_principal_key};
use cratestack_core::{CratestackContext, DEFAULT_BODY_LIMIT_BYTES};

/// Per-socket and per-principal bounds (design §6.5, §6.6).
#[derive(Debug, Clone)]
pub struct WebSocketLimits {
    /// Concurrent subscriptions on one socket. Past it, a new subscription
    /// gets `resource_exhausted` and the socket stays up.
    pub subscriptions_per_socket: usize,
    /// Concurrent sockets per principal (anonymous callers are counted per
    /// remote IP). Past it, the socket closes with 4429.
    pub sockets_per_principal: usize,
    /// Largest message accepted from a client, in bytes; past it, 1009.
    pub max_frame_bytes: usize,
    /// How long a new socket may take to send `Init`.
    pub init_timeout: Duration,
    /// Frames queued for one socket. A client too slow to drain them is
    /// closed with 1013: every subscription is lost, and the client knows.
    pub outbound_queue: usize,
    /// How often the server pings.
    pub ping_interval: Duration,
    /// How long without a pong before the socket is closed.
    pub pong_timeout: Duration,
}

impl Default for WebSocketLimits {
    fn default() -> Self {
        Self {
            subscriptions_per_socket: 64,
            sockets_per_principal: 8,
            max_frame_bytes: DEFAULT_BODY_LIMIT_BYTES,
            init_timeout: Duration::from_secs(10),
            outbound_queue: 1024,
            ping_interval: Duration::from_secs(15),
            pong_timeout: Duration::from_secs(30),
        }
    }
}

/// Derives the identity a socket is counted and re-authenticated under.
/// `None` means anonymous.
pub type PrincipalKeyFn = fn(&CratestackContext) -> Option<String>;

/// Everything the WebSocket route needs.
#[derive(Debug, Clone)]
pub struct WebSocketConfig {
    pub(crate) path: Option<String>,
    pub(crate) origins: OriginPolicy,
    pub(crate) limits: WebSocketLimits,
    pub(crate) principal_key: PrincipalKeyFn,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl WebSocketConfig {
    /// Same-origin browsers only, [`WebSocketLimits::default`], at the
    /// schema's path (`/rpc/ws` for `transport rpc`, `/$ws` for REST).
    pub fn new() -> Self {
        Self {
            path: None,
            origins: OriginPolicy::SameOrigin,
            limits: WebSocketLimits::default(),
            principal_key: default_principal_key,
        }
    }

    /// Serve at `path` instead of the schema's default.
    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// The path, or `default` when none was set.
    pub fn default_path(mut self, default: &str) -> Self {
        self.path.get_or_insert_with(|| default.to_owned());
        self
    }

    pub fn origins(mut self, origins: OriginPolicy) -> Self {
        self.origins = origins;
        self
    }

    pub fn limits(mut self, limits: WebSocketLimits) -> Self {
        self.limits = limits;
        self
    }

    pub fn principal_key(mut self, principal_key: PrincipalKeyFn) -> Self {
        self.principal_key = principal_key;
        self
    }
}
