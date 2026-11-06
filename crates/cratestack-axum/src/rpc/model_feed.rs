//! A `@@subscribe` model feed as a [`SubscriptionStream`]: what the
//! generated `SubscriptionDispatcher` hands every binding — SSE,
//! WebTransport, WebSocket (`docs/design/subscriptions-websocket.md` §6.1).
//!
//! The events come off the subscription backplane ([`model_events`]), so a
//! write committed on any replica reaches a subscriber on this one (§4). Each
//! event is routed per subscriber ([`route_event`]) before it is sent: it
//! passes the subscriber's filter and read policy, or is dropped, or, for an
//! update that moved the row out of both, becomes a filter-exit event (§5.3).
//! The feed never ends on its own, so every ending is an error the binding
//! sends as its terminal frame:
//!
//! - this subscription fell behind its buffer: `unavailable` /
//!   `"subscription lagged"`, the frame the SSE binding has always sent;
//! - the backplane may have lost events (its listener reconnected), or went
//!   away: `unavailable` / `"subscription interrupted"` (§4.5);
//! - routing failed: the policy could not be decided for one event, so that
//!   error, rather than an event nobody checked or a silent gap.

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

/// A model event as it comes off the backplane: the event a subscriber may be
/// sent, and, for an update on an `@@subscribe` model, the row before it. The
/// pre-image stays server-side: it is only ever the `data` of a filter-exit
/// event, after the subscriber's own read check ([`route_event`]).
pub struct FeedEvent<T> {
    pub event: ModelEvent<T>,
    pub before: Option<T>,
}

impl<T: DeserializeOwned> TryFrom<CratestackEventEnvelope> for FeedEvent<T> {
    type Error = CratestackError;

    fn try_from(mut envelope: CratestackEventEnvelope) -> Result<Self, Self::Error> {
        let before = envelope
            .before
            .take()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|error| {
                CratestackError::Codec(format!("failed to decode event pre-image: {error}"))
            })?;
        Ok(Self {
            event: envelope.try_into()?,
            before,
        })
    }
}

/// The model events published on a backplane channel, decoded. A payload
/// that does not decode (a replica on another schema version mid-deploy)
/// ends the feed: the client refetches rather than miss an event.
pub fn model_events<T>(
    receiver: BackplaneReceiver,
) -> impl Stream<Item = Result<FeedEvent<T>, CratestackError>> + Send + 'static
where
    T: DeserializeOwned + Send + 'static,
{
    receiver.map(|message| match message {
        BackplaneMessage::Payload(json) => serde_json::from_str::<CratestackEventEnvelope>(&json)
            .map_err(|error| CratestackError::Codec(format!("unreadable model event: {error}")))
            .and_then(FeedEvent::try_from),
        BackplaneMessage::Lagged => Err(lagged()),
        BackplaneMessage::Resync => Err(interrupted()),
    })
}

/// What one subscriber is sent for `feed`, if anything: the event itself when
/// its row passes the subscriber's filter and read policy (`matches`,
/// `readable`, both on a row); otherwise, for an update, a filter-exit event
/// (`matches: false`, §5.3) if the row before the write had passed both, so
/// the subscriber's copy is not left stale.
///
/// A filter-exit event's `data` is the pre-image and never the post-image: the
/// subscriber already holds that row (it passed the same checks, as a delete
/// event's row does), while the post-image may hold values it may not read,
/// which is why the row left. A row that moved out of the read policy is
/// therefore indistinguishable, to the subscriber, from one that left its
/// filter. An update with no pre-image (a bulk update) yields no exit event.
pub async fn route_event<T, M, R, Fut>(
    feed: FeedEvent<T>,
    wanted: bool,
    matches: M,
    readable: R,
) -> Result<Option<ModelEvent<T>>, CratestackError>
where
    M: Fn(&T) -> bool,
    R: Fn(&T) -> Fut,
    Fut: Future<Output = Result<bool, CratestackError>>,
{
    let FeedEvent { mut event, before } = feed;
    if !wanted {
        return Ok(None);
    }
    // The filter first: it needs no I/O.
    if matches(&event.data) && readable(&event.data).await? {
        return Ok(Some(event));
    }
    let Some(before) = before else {
        return Ok(None);
    };
    if !(matches(&before) && readable(&before).await?) {
        return Ok(None);
    }
    event.data = before;
    event.matches = false;
    Ok(Some(event))
}

/// `items` routed by `route` and encoded to JSON values, ending with the error
/// that stopped it (module docs). `route` returns what to send for an item, or
/// nothing to drop it.
pub fn model_feed<I, O, S, F, Fut>(items: S, route: F) -> SubscriptionStream<serde_json::Value>
where
    O: Serialize + Send + 'static,
    I: Send + 'static,
    S: Stream<Item = Result<I, CratestackError>> + Send + 'static,
    F: FnMut(I) -> Fut + Send + 'static,
    Fut: Future<Output = Result<Option<O>, CratestackError>> + Send + 'static,
{
    let state: (Pin<Box<S>>, F, bool) = (Box::pin(items), route, false);
    Box::pin(stream::unfold(
        state,
        |(mut items, mut route, ended)| async move {
            if ended {
                return None;
            }
            loop {
                let item = match items.next().await {
                    Some(Ok(item)) => item,
                    Some(Err(error)) => return Some((Err(error), (items, route, true))),
                    None => return Some((Err(interrupted()), (items, route, true))),
                };
                let verdict = match route(item).await {
                    Ok(Some(event)) => serde_json::to_value(&event).map_err(|error| {
                        CratestackError::Codec(format!("failed to encode a model event: {error}"))
                    }),
                    Ok(None) => continue,
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
                return Some((verdict, (items, route, ended)));
            }
        },
    ))
}

#[cfg(test)]
mod tests;
