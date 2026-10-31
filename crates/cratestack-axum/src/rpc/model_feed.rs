//! A `@@subscribe` model feed as a [`SubscriptionStream`]: what the
//! generated `SubscriptionDispatcher` hands every binding — SSE,
//! WebTransport, later WebSocket (`docs/design/subscriptions-websocket.md`
//! §6.1, slice S5a).
//!
//! The events come from [`super::guarded_receiver_stream`]. Each one passes
//! the subscriber's read policy (`readable`) before it is sent; one the
//! subscriber may not read is dropped. The feed itself never ends on its
//! own, so both of its endings are errors the binding sends as its terminal
//! frame:
//!
//! - the receiver ended: the bounded channel overflowed
//!   ([`super::subscription_bridge`]), so `unavailable` / `"subscription
//!   lagged"` — the frame the SSE binding has always sent for it;
//! - `readable` failed: the policy could not be decided for one event, so
//!   that error, rather than an event nobody checked or a silent gap.

use std::future::Future;
use std::pin::Pin;

use cratestack_core::{CratestackError, SubscriptionStream};
use futures_util::Stream;
use futures_util::stream::{self, StreamExt};
use serde::Serialize;

/// The error a feed ends with when its buffer overflowed.
pub(crate) fn lagged() -> CratestackError {
    CratestackError::Unavailable("subscription lagged".to_owned())
}

/// `items` filtered by `readable` and encoded to JSON values, ending with
/// the error that stopped it (module docs).
pub fn model_feed<T, S, F, Fut>(items: S, readable: F) -> SubscriptionStream<serde_json::Value>
where
    T: Serialize + Send + 'static,
    S: Stream<Item = T> + Send + 'static,
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
                let Some(item) = items.next().await else {
                    return Some((Err(lagged()), (items, readable, true)));
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
