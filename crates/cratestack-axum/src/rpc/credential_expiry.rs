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
pub fn until_credential_expires<T: Send + 'static>(
    items: SubscriptionStream<T>,
    expires_at: Option<i64>,
) -> SubscriptionStream<T> {
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
            // The clock first: `select` polls its first argument first, so a
            // source that is always ready cannot starve the expiry.
            match select(deadline.as_mut(), items.next()).await {
                Either::Left(((), _)) => Some((Err(expired()), (items, deadline, true))),
                Either::Right((Some(item), _)) => {
                    let ended = item.is_err();
                    Some((item, (items, deadline, ended)))
                }
                Either::Right((None, _)) => None,
            }
        },
    ))
}

#[cfg(test)]
mod tests;
