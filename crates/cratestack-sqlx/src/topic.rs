//! A schema `topic`'s runtime handle on `db = Postgres`
//! (`docs/design/subscriptions-websocket.md` §3.4, slice S4): the
//! backplane-only [`TopicHandle`] from `cratestack-core`, plus
//! `publish_in_tx`, which sends from inside a transaction so the message is
//! delivered only if it commits, in commit order with the transaction's model
//! events. It needs a backplane the transaction can notify
//! (`PgNotifyBackplane`, the default).
//!
//! A topic taken from an `@isolation` procedure's handle is bound to the
//! procedure's attempt: its `publish` is delivered only if the attempt
//! commits, whichever backplane carries it (`PgNotifyBackplane` notifies
//! inside the transaction, any other sends after the commit).

use std::fmt::Display;
use std::ops::Deref;
use std::sync::Arc;

use cratestack_core::{CratestackError, SubscriptionBackplane, TopicHandle};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::SqlxRuntime;
use crate::backplane::notify_payload::{NOTIFY_CHANNEL, encode};
use crate::bound::{BoundTx, nested_in_bound};
use crate::error::cratestack_error_from_sqlx;
use crate::sqlx;

/// One topic under one key. Cheap to build; hold none. `subscribe` and
/// `channel` are the [`TopicHandle`]'s; `publish` is this type's own, so a
/// topic bound to an `@isolation` attempt rides it.
pub struct Topic<T> {
    handle: TopicHandle<T>,
    /// The `@isolation` attempt a `publish` rides, when there is one.
    attempt: Option<Arc<BoundTx>>,
}

impl<T> Deref for Topic<T> {
    type Target = TopicHandle<T>;

    fn deref(&self) -> &Self::Target {
        &self.handle
    }
}

impl<T> Topic<T>
where
    T: Serialize + DeserializeOwned + Send + 'static,
{
    /// `topic` is the declared name (`RoomMessage`), `key` the key's value.
    pub fn new(backplane: Arc<dyn SubscriptionBackplane>, topic: &str, key: impl Display) -> Self {
        Self {
            handle: TopicHandle::new(backplane, topic, key),
            attempt: None,
        }
    }

    /// Binds `publish` to `runtime`'s `@isolation` attempt. A runtime that
    /// is not one, or none, leaves the topic as it was.
    #[doc(hidden)]
    pub fn in_attempt(mut self, runtime: Option<&SqlxRuntime>) -> Self {
        self.attempt = runtime.and_then(|runtime| runtime.bound.clone());
        self
    }

    /// Delivers `message` to every current subscriber, on every replica.
    /// From an `@isolation` procedure's handle it is delivered only if the
    /// attempt commits, never for one that rolls back or retries: with
    /// `PgNotifyBackplane` as [`Self::publish_in_tx`] in a savepoint of the
    /// attempt, with any other backplane queued and sent after the commit.
    /// Inside `db.transaction(..)` there, use `publish_in_tx(tx, ..)`.
    pub async fn publish(&self, message: &T) -> Result<(), CratestackError> {
        let Some(attempt) = &self.attempt else {
            return self.handle.publish(message).await;
        };
        let payload = self.encode(message)?;
        if !self.backplane().publishes_model_events_in_tx() {
            attempt.defer_publish(self.backplane().clone(), self.channel().to_owned(), payload);
            return Ok(());
        }
        attempt.lock().map(drop).map_err(|_| {
            CratestackError::Internal(
                "a topic publish is refused while `db.transaction(..)` runs on this handle: \
                 call `publish_in_tx(tx, &message)` with its `tx`"
                    .to_owned(),
            )
        })?;
        nested_in_bound(attempt, async |tx| self.notify(tx, &payload).await).await
    }

    /// Delivers `message` only if `tx` commits, when it commits. On a topic
    /// from an `@isolation` procedure's handle, a backplane the transaction
    /// cannot notify gets it queued until the attempt commits (dropped if
    /// this `db.transaction(..)` rolls back). Otherwise such a backplane
    /// (`InProcessBackplane`) is refused: call [`Self::publish`] after the
    /// commit instead.
    pub async fn publish_in_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        message: &T,
    ) -> Result<(), CratestackError> {
        if !self.backplane().publishes_model_events_in_tx() {
            if let Some(attempt) = &self.attempt {
                let payload = self.encode(message)?;
                attempt.defer_publish(self.backplane().clone(), self.channel().to_owned(), payload);
                return Ok(());
            }
            return Err(CratestackError::Internal(
                "`publish_in_tx` needs a backplane the transaction notifies (PgNotifyBackplane); \
                 with this one, `publish` after the commit"
                    .to_owned(),
            ));
        }
        self.notify(tx, &self.encode(message)?).await
    }

    /// `pg_notify` of `payload` inside `tx`.
    async fn notify(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        payload: &str,
    ) -> Result<(), CratestackError> {
        let notification = encode(self.channel(), payload, None).ok_or_else(|| {
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
}
