//! A schema `topic`'s runtime handle (`docs/design/subscriptions-websocket.md`
//! §3.4, slice S4): typed messages under one key, carried to every replica
//! by the subscription backplane.
//!
//! The generated `db.topics().room_message(&room_id)` returns one, exported
//! as `cratestack::Topic` by `cratestack-pg` (whose `Topic` adds
//! `publish_in_tx`, which needs a Postgres transaction) and `cratestack-api`.
//! `publish` sends now; `subscribe` is what a `subscription procedure`
//! handler usually returns.

use std::fmt::Display;
use std::marker::PhantomData;
use std::sync::Arc;

use futures_util::stream::{self, StreamExt};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::{BackplaneMessage, SubscriptionBackplane, topic_channel};
use crate::error::CratestackError;
use crate::subscription::SubscriptionStream;

/// One topic under one key. Cheap to build; hold none.
pub struct TopicHandle<T> {
    backplane: Arc<dyn SubscriptionBackplane>,
    channel: String,
    message: PhantomData<fn() -> T>,
}

impl<T> TopicHandle<T>
where
    T: Serialize + DeserializeOwned + Send + 'static,
{
    /// `topic` is the declared name (`RoomMessage`), `key` the key's value.
    pub fn new(backplane: Arc<dyn SubscriptionBackplane>, topic: &str, key: impl Display) -> Self {
        Self {
            backplane,
            channel: topic_channel(topic, &key.to_string()),
            message: PhantomData,
        }
    }

    /// The backplane channel: `cratestack.topic.<Topic>.<key>`.
    pub fn channel(&self) -> &str {
        &self.channel
    }

    /// The backplane the topic publishes on.
    pub fn backplane(&self) -> &Arc<dyn SubscriptionBackplane> {
        &self.backplane
    }

    /// `message` as the JSON payload the backplane carries.
    pub fn encode(&self, message: &T) -> Result<String, CratestackError> {
        serde_json::to_string(message).map_err(|error| {
            CratestackError::Codec(format!("failed to encode a topic message: {error}"))
        })
    }

    /// Delivers `message` to every current subscriber, on every replica.
    pub async fn publish(&self, message: &T) -> Result<(), CratestackError> {
        let payload = self.encode(message)?;
        self.backplane.publish(&self.channel, &payload).await
    }

    /// Every message published from now on. Resolves once the subscription
    /// is live. Ends with `unavailable` when it falls behind
    /// (`"subscription lagged"`) or the backplane may have lost messages
    /// (`"subscription interrupted"`): resubscribe and refetch.
    pub async fn subscribe(&self) -> Result<SubscriptionStream<T>, CratestackError> {
        let receiver = self.backplane.subscribe(&self.channel).await?;
        Ok(Box::pin(stream::unfold(
            Some(receiver),
            |receiver| async move {
                let mut receiver = receiver?;
                let item = match receiver.next().await? {
                    BackplaneMessage::Payload(json) => {
                        serde_json::from_str(&json).map_err(|error| {
                            CratestackError::Codec(format!("unreadable topic message: {error}"))
                        })
                    }
                    BackplaneMessage::Lagged => Err(unavailable("subscription lagged")),
                    BackplaneMessage::Resync => Err(unavailable("subscription interrupted")),
                };
                let rest = item.is_ok().then_some(receiver);
                Some((item, rest))
            },
        )))
    }
}

fn unavailable(message: &str) -> CratestackError {
    CratestackError::Unavailable(message.to_owned())
}
