//! The subscription backplane (`docs/design/subscriptions-websocket.md` §4):
//! how a published message reaches every replica, so a subscriber connected
//! to one replica sees a write committed on another.
//!
//! This module holds the contract only. The implementations live where their
//! runtime does: `InProcessBackplane` (one replica) and `PgNotifyBackplane`
//! (Postgres `LISTEN`/`NOTIFY`) in `cratestack-sqlx`.
//!
//! Delivery is at-most-once (§2.1). A backplane that may have lost messages
//! says so with [`BackplaneMessage::Resync`], and every subscription on that
//! channel ends with `unavailable`, so the client knows to refetch rather than
//! trust a stream with a silent gap.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use futures_core::Stream;

use crate::error::CratestackError;

/// One delivery on a backplane channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackplaneMessage {
    /// A published payload (JSON text, as published).
    Payload(Arc<str>),
    /// Messages may have been lost: the backplane's listener reconnected. The
    /// subscription must end so its client resubscribes.
    Resync,
    /// This receiver fell behind by more than its buffer and messages were
    /// dropped for it alone. The subscription ends, as with `Resync`.
    Lagged,
}

/// What [`SubscriptionBackplane::subscribe`] returns: everything published on
/// the channel from the moment the subscription is established.
pub type BackplaneReceiver = Pin<Box<dyn Stream<Item = BackplaneMessage> + Send + 'static>>;

/// A boxed future, so the trait stays object-safe (`Arc<dyn SubscriptionBackplane>`).
pub type BackplaneFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The backplane channel model events of `model` are published on.
pub fn model_event_channel(model: &str) -> String {
    format!("cratestack.model.{model}")
}

/// Fans a published message out to every replica, this one included.
pub trait SubscriptionBackplane: Send + Sync + 'static {
    /// Publishes `payload` on `channel`, fire and forget.
    fn publish<'a>(
        &'a self,
        channel: &'a str,
        payload: &'a str,
    ) -> BackplaneFuture<'a, Result<(), CratestackError>>;

    /// Receives everything published on `channel` from now on. The future
    /// resolves only once the subscription is live, so a caller that then
    /// tells its client "ready" has not missed anything published after.
    fn subscribe<'a>(
        &'a self,
        channel: &'a str,
    ) -> BackplaneFuture<'a, Result<BackplaneReceiver, CratestackError>>;

    /// Whether model events reach this backplane from inside the writing
    /// transaction (Postgres `NOTIFY`, delivered at commit). When `false`, the
    /// runtime publishes each model event after commit, from its outbox drain.
    fn publishes_model_events_in_tx(&self) -> bool {
        false
    }
}
