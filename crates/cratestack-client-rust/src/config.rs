use reqwest::Url;

use crate::subscription::SubscriptionTransport;

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub base_url: Url,
    /// Which binding subscriptions use (`docs/design/subscriptions-websocket.md`
    /// §8). Default: [`SubscriptionTransport::Auto`].
    pub subscription_transport: SubscriptionTransport,
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
            #[cfg(all(feature = "webtransport", not(target_arch = "wasm32")))]
            webtransport: None,
        }
    }

    /// Pin subscriptions to one binding instead of `Auto`.
    pub fn with_subscription_transport(mut self, transport: SubscriptionTransport) -> Self {
        self.subscription_transport = transport;
        self
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
