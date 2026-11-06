//! A schema `topic`'s runtime handle (`docs/design/subscriptions-websocket.md`
//! §3.4, slice S4): typed messages under one key, carried to every replica
//! by the subscription backplane.
//!
//! The generated `db.topics().room_message(&room_id)` returns a [`Topic`].
//! `publish` sends now. `publish_in_tx` sends from inside a transaction, so
//! the message is delivered only if it commits, in commit order with the
//! transaction's model events; it needs a backplane the transaction can
//! notify (`PgNotifyBackplane`, the default). `subscribe` is what a
//! `subscription procedure` handler usually returns.
//!
//! A topic taken from an `@isolation` procedure's handle is bound to the
//! procedure's attempt: its `publish` is `publish_in_tx` on that
//! transaction (`docs/design/procedure-isolation.md` §3).

use std::fmt::Display;
use std::marker::PhantomData;
use std::sync::Arc;

use cratestack_core::{
    BackplaneMessage, CratestackError, SubscriptionBackplane, SubscriptionStream, topic_channel,
};
use futures_util::stream::{self, StreamExt};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::SqlxRuntime;
use crate::backplane::notify_payload::{NOTIFY_CHANNEL, encode};
use crate::error::cratestack_error_from_sqlx;
use crate::sqlx;

/// One topic under one key. Cheap to build; hold none.
pub struct Topic<T> {
    backplane: Arc<dyn SubscriptionBackplane>,
    channel: String,
    /// The `@isolation` attempt a `publish` rides, when there is one.
    attempt: Option<SqlxRuntime>,
    message: PhantomData<fn() -> T>,
}

impl<T> Topic<T>
where
    T: Serialize + DeserializeOwned + Send + 'static,
{
    /// `topic` is the declared name (`RoomMessage`), `key` the key's value.
    pub fn new(backplane: Arc<dyn SubscriptionBackplane>, topic: &str, key: impl Display) -> Self {
        Self {
            backplane,
            channel: topic_channel(topic, &key.to_string()),
            attempt: None,
            message: PhantomData,
        }
    }

    /// Binds `publish` to `runtime`'s transaction when it is an `@isolation`
    /// attempt's runtime; any other runtime leaves the topic as it was.
    pub fn bound_to(mut self, runtime: &SqlxRuntime) -> Self {
        self.attempt = runtime.bound().is_some().then(|| runtime.clone());
        self
    }

    /// The backplane channel: `cratestack.topic.<Topic>.<key>`.
    pub fn channel(&self) -> &str {
        &self.channel
    }

    /// Delivers `message` to every current subscriber, on every replica.
    /// From an `@isolation` procedure's handle it is [`Self::publish_in_tx`]
    /// on the procedure's transaction instead: delivered if the attempt
    /// commits, never for one that rolls back or retries.
    pub async fn publish(&self, message: &T) -> Result<(), CratestackError> {
        if let Some(runtime) = &self.attempt {
            return runtime
                .transaction(async |tx| self.publish_in_tx(tx, message).await)
                .await;
        }
        let payload = encode_message(message)?;
        self.backplane.publish(&self.channel, &payload).await
    }

    /// Delivers `message` only if `tx` commits, when it commits. Refused
    /// with a backplane the transaction cannot notify (`InProcessBackplane`):
    /// call [`Self::publish`] after the commit instead.
    pub async fn publish_in_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        message: &T,
    ) -> Result<(), CratestackError> {
        if !self.backplane.publishes_model_events_in_tx() {
            return Err(CratestackError::Internal(
                "publishing in a transaction needs a backplane the transaction notifies \
                 (PgNotifyBackplane); with this one, `publish` after the commit"
                    .to_owned(),
            ));
        }
        let payload = encode_message(message)?;
        let notification = encode(&self.channel, &payload, None).ok_or_else(|| {
            CratestackError::BadRequest(
                "a topic message must stay under Postgres' 8000-byte NOTIFY limit".to_owned(),
            )
        })?;
        sqlx::query("SELECT pg_notify($1, $2)")
            .bind(NOTIFY_CHANNEL)
            .bind(notification)
            .execute(&mut **tx)
            .await
            .map_err(cratestack_error_from_sqlx)?;
        Ok(())
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

fn encode_message<T: Serialize>(message: &T) -> Result<String, CratestackError> {
    serde_json::to_string(message).map_err(|error| {
        CratestackError::Codec(format!("failed to encode a topic message: {error}"))
    })
}

fn unavailable(message: &str) -> CratestackError {
    CratestackError::Unavailable(message.to_owned())
}

#[cfg(test)]
mod tests;
