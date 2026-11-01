//! A `@@subscribe` model feed as a [`SubscriptionStream`]: what the
//! generated `SubscriptionDispatcher` hands every binding — SSE,
//! WebTransport, WebSocket (`docs/design/subscriptions-websocket.md` §6.1).
//!
//! The events come off the subscription backplane ([`model_events`]), so a
//! write committed on any replica reaches a subscriber on this one (§4). Each
//! event passes the subscriber's read policy (`readable`) before it is sent;
//! one the subscriber may not read is dropped. The feed never ends on its own,
//! so every ending is an error the binding sends as its terminal frame:
//!
//! - this subscription fell behind its buffer: `unavailable` /
//!   `"subscription lagged"`, the frame the SSE binding has always sent;
//! - the backplane may have lost events (its listener reconnected), or went
//!   away: `unavailable` / `"subscription interrupted"` (§4.5);
//! - `readable` failed: the policy could not be decided for one event, so
//!   that error, rather than an event nobody checked or a silent gap.

use std::future::Future;
use std::pin::Pin;

use cratestack_core::{
    BackplaneMessage, BackplaneReceiver, CratestackError, CratestackEventEnvelope, ModelEvent,
    SubscriptionStream,
};
use futures_util::Stream;
use futures_util::stream::{self, StreamExt};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// The error a feed ends with when its buffer overflowed.
pub(crate) fn lagged() -> CratestackError {
    CratestackError::Unavailable("subscription lagged".to_owned())
}

fn interrupted() -> CratestackError {
    CratestackError::Unavailable("subscription interrupted".to_owned())
}

/// The model events published on a backplane channel, decoded. A payload
/// that does not decode (a replica on another schema version mid-deploy)
/// ends the feed: the client refetches rather than miss an event.
pub fn model_events<T>(
    receiver: BackplaneReceiver,
) -> impl Stream<Item = Result<ModelEvent<T>, CratestackError>> + Send + 'static
where
    T: DeserializeOwned + Send + 'static,
{
    receiver.map(|message| match message {
        BackplaneMessage::Payload(json) => serde_json::from_str::<CratestackEventEnvelope>(&json)
            .map_err(|error| CratestackError::Codec(format!("unreadable model event: {error}")))
            .and_then(ModelEvent::try_from),
        BackplaneMessage::Lagged => Err(lagged()),
        BackplaneMessage::Resync => Err(interrupted()),
    })
}

/// `items` filtered by `readable` and encoded to JSON values, ending with
/// the error that stopped it (module docs).
pub fn model_feed<T, S, F, Fut>(items: S, readable: F) -> SubscriptionStream<serde_json::Value>
where
    T: Serialize + Send + 'static,
    S: Stream<Item = Result<T, CratestackError>> + Send + 'static,
    F: FnMut(&T) -> Fut + Send + 'static,
    Fut: Future<Output = Result<bool, CratestackError>> + Send + 'static,
{
    let state: (Pin<Box<S>>, F, bool) = (Box::pin(items), readable, false);
    Box::pin(stream::unfold(
        state,
        |(mut items, mut readable, ended)| async move {
            if ended {
                return None;
            }
            loop {
                let item = match items.next().await {
                    Some(Ok(item)) => item,
                    Some(Err(error)) => return Some((Err(error), (items, readable, true))),
                    None => return Some((Err(interrupted()), (items, readable, true))),
                };
                let verdict = match readable(&item).await {
                    Ok(true) => serde_json::to_value(&item).map_err(|error| {
                        CratestackError::Codec(format!("failed to encode a model event: {error}"))
                    }),
                    Ok(false) => continue,
                    Err(error) => {
                        tracing::warn!(
                            target: "cratestack",
                            %error,
                            "subscription read policy undecided; ending the stream",
                        );
                        Err(error)
                    }
                };
                let ended = verdict.is_err();
                return Some((verdict, (items, readable, ended)));
            }
        },
    ))
}

#[cfg(test)]
mod tests;
