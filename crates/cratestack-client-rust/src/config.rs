use reqwest::Url;

use crate::subscription::SubscriptionTransport;

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub base_url: Url,
    /// Which binding subscriptions use (`docs/design/subscriptions-websocket.md`
    /// §8). Default: [`SubscriptionTransport::Auto`].
    pub subscription_transport: SubscriptionTransport,
}

impl ClientConfig {
    pub fn new(base_url: Url) -> Self {
        Self {
            base_url,
            subscription_transport: SubscriptionTransport::default(),
        }
    }

    /// Pin subscriptions to one binding instead of `Auto`.
    pub fn with_subscription_transport(mut self, transport: SubscriptionTransport) -> Self {
        self.subscription_transport = transport;
        self
    }
}
