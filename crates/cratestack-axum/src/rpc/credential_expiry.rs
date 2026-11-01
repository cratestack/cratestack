//! A subscription lives no longer than the credential that opened it
//! (`docs/design/subscriptions-websocket.md` §6.3): the context captured at
//! open is used for every event, so once its `exp` passes the stream ends
//! with `unauthenticated` / `"credential expired"` and the client
//! resubscribes with a fresh credential. A context without `exp` never
//! expires here.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cratestack_core::{CratestackError, SubscriptionStream};
use futures_util::future::{Either, select};
use futures_util::stream::{self, StreamExt};

fn expired() -> CratestackError {
    CratestackError::Unauthorized("credential expired".to_owned())
}

/// `items`, ended with `unauthenticated` at `expires_at` (seconds since the
/// epoch, the `exp` claim). Already expired: the first item is that error.
pub fn until_credential_expires(
    items: SubscriptionStream<serde_json::Value>,
    expires_at: Option<i64>,
) -> SubscriptionStream<serde_json::Value> {
    let Some(expires_at) = expires_at else {
        return items;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0);
    let remaining = Duration::from_secs(expires_at.saturating_sub(now).max(0) as u64);
    let deadline = tokio::time::Instant::now() + remaining;
    let state = (items, Box::pin(tokio::time::sleep_until(deadline)), false);
    Box::pin(stream::unfold(
        state,
        |(mut items, mut deadline, ended)| async move {
            if ended {
                return None;
            }
            match select(items.next(), deadline.as_mut()).await {
                Either::Left((Some(item), _)) => {
                    let ended = item.is_err();
                    Some((item, (items, deadline, ended)))
                }
                Either::Left((None, _)) => None,
                Either::Right(((), _)) => Some((Err(expired()), (items, deadline, true))),
            }
        },
    ))
}

#[cfg(test)]
mod tests;
