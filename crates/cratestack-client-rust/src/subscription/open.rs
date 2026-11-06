//! Which binding opens a subscription: the configured one, or `Auto`'s
//! chain. Each binding this build lacks answers with a `BadInput` naming the
//! feature it needs.

use serde::de::DeserializeOwned;

use super::failure::Failure;
use super::{ClientSubscription, SubscriptionTarget, SubscriptionTransport, sse};
use crate::client::CratestackClient;
use crate::codec::HttpClientCodec;
use crate::error::ClientError;

const WT: bool = cfg!(all(feature = "webtransport", not(target_arch = "wasm32")));
const WS: bool = cfg!(all(feature = "ws", not(target_arch = "wasm32")));

/// How long `Auto` waits for a WebTransport session before moving on: a
/// UDP-blocked network otherwise waits for the QUIC idle timeout.
#[cfg(all(feature = "webtransport", not(target_arch = "wasm32")))]
const AUTO_WT_WITHIN: std::time::Duration = std::time::Duration::from_secs(3);

impl<C: HttpClientCodec> CratestackClient<C> {
    /// Opens `target`, resolving once the server says `Ready`.
    pub async fn subscribe<T>(
        &self,
        target: SubscriptionTarget,
    ) -> Result<ClientSubscription<T>, ClientError>
    where
        T: DeserializeOwned + Send + 'static,
    {
        match self.config.subscription_transport {
            SubscriptionTransport::Sse => sse::open(self, &target).await,
            SubscriptionTransport::WebSocket => {
                self.open_ws(&target).await.map_err(Failure::into_error)
            }
            SubscriptionTransport::WebTransport => self
                .open_wt(&target, false)
                .await
                .map_err(Failure::into_error),
            SubscriptionTransport::Auto => self.open_auto(&target).await,
        }
    }

    async fn open_auto<T: DeserializeOwned + Send + 'static>(
        &self,
        target: &SubscriptionTarget,
    ) -> Result<ClientSubscription<T>, ClientError> {
        if WT && self.webtransport_configured() {
            match self.open_wt(target, true).await {
                Ok(subscription) => return Ok(subscription),
                Err(Failure::Refused(error)) => return Err(error),
                Err(Failure::Unreachable(_)) => {}
            }
        }
        if WS {
            match self.open_ws(target).await {
                Ok(subscription) => return Ok(subscription),
                Err(Failure::Refused(error)) => return Err(error),
                Err(Failure::Unreachable(_)) => {}
            }
        }
        sse::open(self, target).await
    }

    #[cfg(all(feature = "webtransport", not(target_arch = "wasm32")))]
    fn webtransport_configured(&self) -> bool {
        self.config
            .webtransport
            .as_ref()
            .is_some_and(|endpoint| !endpoint.recently_failed())
    }

    #[cfg(not(all(feature = "webtransport", not(target_arch = "wasm32"))))]
    fn webtransport_configured(&self) -> bool {
        false
    }

    /// `auto`: bounded by [`AUTO_WT_WITHIN`], and a failure is remembered.
    #[cfg(all(feature = "webtransport", not(target_arch = "wasm32")))]
    async fn open_wt<T: DeserializeOwned + Send + 'static>(
        &self,
        target: &SubscriptionTarget,
        auto: bool,
    ) -> Result<ClientSubscription<T>, Failure> {
        let Some(endpoint) = self.config.webtransport.as_ref() else {
            return Err(Failure::Refused(ClientError::BadInput(
                "WebTransport subscriptions need `ClientConfig::with_webtransport`".to_owned(),
            )));
        };
        let connect = super::wt::connect(self, endpoint);
        let session = if auto {
            match tokio::time::timeout(AUTO_WT_WITHIN, connect).await {
                Ok(session) => session,
                Err(_) => Err(Failure::Unreachable(ClientError::InvalidResponse(
                    "WebTransport: no session in time".to_owned(),
                ))),
            }
        } else {
            connect.await
        };
        let session = session.inspect_err(|failure| {
            if matches!(failure, Failure::Unreachable(_)) {
                endpoint.mark_failed();
            }
        })?;
        super::wt::open(session, target).await
    }

    #[cfg(not(all(feature = "webtransport", not(target_arch = "wasm32"))))]
    async fn open_wt<T: DeserializeOwned + Send + 'static>(
        &self,
        _target: &SubscriptionTarget,
        _auto: bool,
    ) -> Result<ClientSubscription<T>, Failure> {
        Err(Failure::Refused(ClientError::BadInput(
            "WebTransport subscriptions need the `webtransport` feature on a native target"
                .to_owned(),
        )))
    }

    #[cfg(all(feature = "ws", not(target_arch = "wasm32")))]
    async fn open_ws<T: DeserializeOwned + Send + 'static>(
        &self,
        target: &SubscriptionTarget,
    ) -> Result<ClientSubscription<T>, Failure> {
        super::ws::open(self, target).await
    }

    #[cfg(not(all(feature = "ws", not(target_arch = "wasm32"))))]
    async fn open_ws<T: DeserializeOwned + Send + 'static>(
        &self,
        _target: &SubscriptionTarget,
    ) -> Result<ClientSubscription<T>, Failure> {
        Err(Failure::Refused(ClientError::BadInput(
            "WebSocket subscriptions need the `ws` feature on a native target".to_owned(),
        )))
    }
}
