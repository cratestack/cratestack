use reqwest::Url;

use crate::subscription::SubscriptionTransport;

/// The default per-event cap for SSE subscriptions: 1 MiB. The
/// `application/cbor-seq` binding applies the same cap to one frame.
pub const DEFAULT_MAX_SSE_EVENT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub base_url: Url,
    /// Which binding subscriptions use (`docs/design/subscriptions-websocket.md`
    /// §8). Default: [`SubscriptionTransport::Auto`].
    pub subscription_transport: SubscriptionTransport,
    /// The most bytes one server-sent event may span (cratestack#1379),
    /// counted as raw bytes received, line endings and the terminating blank
    /// line included. The generated TypeScript and Dart clients count UTF-8
    /// bytes after `\r\n` -> `\n` normalization, so they may accept up to one
    /// more byte per line; the bound is linear in the cap either way. A longer event ends the subscription with
    /// [`ClientError::EventTooLarge`](crate::ClientError::EventTooLarge).
    /// Default: [`DEFAULT_MAX_SSE_EVENT_BYTES`].
    ///
    /// The `application/cbor-seq` binding reads this too: one CBOR frame may
    /// span at most this many bytes. The name stays for compatibility;
    /// [`ClientConfig::with_max_subscription_frame_bytes`] sets it under
    /// the name that fits both.
    pub max_sse_event_bytes: usize,
    /// The server's WebTransport listener, a separate UDP address (feature
    /// `webtransport`, native targets). Without it `Auto` starts at the
    /// WebSocket.
    #[cfg(all(feature = "webtransport", not(target_arch = "wasm32")))]
    pub webtransport: Option<WebTransportEndpoint>,
}

impl ClientConfig {
    pub fn new(base_url: Url) -> Self {
        Self {
            base_url,
            subscription_transport: SubscriptionTransport::default(),
            max_sse_event_bytes: DEFAULT_MAX_SSE_EVENT_BYTES,
            #[cfg(all(feature = "webtransport", not(target_arch = "wasm32")))]
            webtransport: None,
        }
    }

    /// Pin subscriptions to one binding instead of `Auto`.
    pub fn with_subscription_transport(mut self, transport: SubscriptionTransport) -> Self {
        self.subscription_transport = transport;
        self
    }

    /// Cap one SSE event at `bytes` (default [`DEFAULT_MAX_SSE_EVENT_BYTES`]).
    pub fn with_max_sse_event_bytes(mut self, bytes: usize) -> Self {
        self.max_sse_event_bytes = bytes;
        self
    }

    /// The same cap as [`Self::with_max_sse_event_bytes`], under the name that
    /// covers both HTTP bindings: the most bytes one SSE event or one
    /// `application/cbor-seq` frame may span.
    pub fn with_max_subscription_frame_bytes(self, bytes: usize) -> Self {
        self.with_max_sse_event_bytes(bytes)
    }

    /// Subscribe over WebTransport at `url` (`https://host:4433/rpc/wt`, or
    /// `/$wt` on a REST schema), trusting `trust`. `Auto` then tries it
    /// first.
    #[cfg(all(feature = "webtransport", not(target_arch = "wasm32")))]
    pub fn with_webtransport(mut self, url: Url, trust: crate::ServerTrust) -> Self {
        self.webtransport = Some(WebTransportEndpoint::new(url, trust));
        self
    }
}

#[cfg(all(feature = "webtransport", not(target_arch = "wasm32")))]
pub use webtransport_endpoint::WebTransportEndpoint;

#[cfg(all(feature = "webtransport", not(target_arch = "wasm32")))]
mod webtransport_endpoint {
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use reqwest::Url;

    /// How long `Auto` skips WebTransport after a session failed to open,
    /// so a UDP-blocked network pays the wait once (as the TypeScript
    /// client does).
    const REMEMBER_FAILURE: Duration = Duration::from_secs(600);

    /// Where the WebTransport listener is, and which certificate it shows.
    #[derive(Debug, Clone)]
    pub struct WebTransportEndpoint {
        pub url: Url,
        pub trust: crate::ServerTrust,
        /// Shared by every clone of the config.
        failed_at: Arc<Mutex<Option<Instant>>>,
    }

    impl WebTransportEndpoint {
        pub fn new(url: Url, trust: crate::ServerTrust) -> Self {
            Self {
                url,
                trust,
                failed_at: Arc::default(),
            }
        }

        pub(crate) fn recently_failed(&self) -> bool {
            let failed_at = self.failed_at.lock().unwrap_or_else(|e| e.into_inner());
            failed_at.is_some_and(|at| at.elapsed() < REMEMBER_FAILURE)
        }

        pub(crate) fn mark_failed(&self) {
            *self.failed_at.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
        }
    }
}
