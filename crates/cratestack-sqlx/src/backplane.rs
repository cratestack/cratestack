//! Subscription backplanes (`docs/design/subscriptions-websocket.md` §4):
//! [`InProcessBackplane`] for one replica, [`PgNotifyBackplane`] for many.
//!
//! Both fan out locally the same way ([`LocalHub`]): one bounded broadcast
//! channel per backplane channel with at least one subscriber, each receiver a
//! subscription. A receiver that falls behind by more than the capacity gets
//! [`BackplaneMessage::Lagged`] and its subscription ends, the bounded-buffer
//! rule the SSE binding has always had (64 items).

mod in_process;
pub(crate) mod notify_payload;
mod pg_notify;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use cratestack_core::{BackplaneMessage, BackplaneReceiver};
use tokio::sync::broadcast;

pub use in_process::InProcessBackplane;
pub use pg_notify::PgNotifyBackplane;

/// Items a subscription may fall behind by before it is ended as lagged.
pub const SUBSCRIPTION_BUFFER_CAPACITY: usize = 64;

#[derive(Default)]
pub(crate) struct LocalHub {
    channels: Mutex<HashMap<String, broadcast::Sender<BackplaneMessage>>>,
}

impl LocalHub {
    fn lock(
        &self,
    ) -> std::sync::MutexGuard<'_, HashMap<String, broadcast::Sender<BackplaneMessage>>> {
        self.channels.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn subscribe(&self, channel: &str) -> BackplaneReceiver {
        let receiver = self
            .lock()
            .entry(channel.to_owned())
            .or_insert_with(|| broadcast::channel(SUBSCRIPTION_BUFFER_CAPACITY).0)
            .subscribe();
        receiver_stream(receiver)
    }

    /// Delivers to `channel`'s subscribers; forgets the channel once it has none.
    pub(crate) fn deliver(&self, channel: &str, payload: Arc<str>) {
        let mut channels = self.lock();
        if let Some(sender) = channels.get(channel)
            && sender.send(BackplaneMessage::Payload(payload)).is_err()
        {
            channels.remove(channel);
        }
    }

    /// Tells `channel`'s subscribers that messages may have been lost.
    pub(crate) fn resync(&self, channel: &str) {
        if let Some(sender) = self.lock().remove(channel) {
            let _ = sender.send(BackplaneMessage::Resync);
        }
    }

    /// Tells every subscriber that messages may have been lost.
    pub(crate) fn resync_all(&self) {
        self.lock()
            .retain(|_, sender| sender.send(BackplaneMessage::Resync).is_ok());
    }
}

fn receiver_stream(receiver: broadcast::Receiver<BackplaneMessage>) -> BackplaneReceiver {
    Box::pin(futures_util::stream::unfold(
        Some(receiver),
        |receiver| async move {
            let mut receiver = receiver?;
            match receiver.recv().await {
                Ok(message @ (BackplaneMessage::Resync | BackplaneMessage::Lagged)) => {
                    Some((message, None))
                }
                Ok(message) => Some((message, Some(receiver))),
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    Some((BackplaneMessage::Lagged, None))
                }
                Err(broadcast::error::RecvError::Closed) => None,
            }
        },
    ))
}

#[cfg(test)]
mod tests;
