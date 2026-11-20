//! The longest a signed subscription may stream (cratestack#1378).
//!
//! A signed open authenticates only the request; the frames that follow are
//! plain, so the server bounds how long one open stays alive. The envelope
//! layer puts a [`SignedStreamDeadline`] in the request's extensions, and
//! [`serve_subscription_with`](super::serve_subscription_with) ends the
//! stream with `unavailable` / `"stream lifetime"` when it passes; the client
//! resubscribes with a fresh signed open. The credential's own `exp`
//! ([`until_credential_expires`](super::until_credential_expires)) is applied
//! first, so whichever comes first wins.

use cratestack_core::{CratestackError, StreamEndNotice, StreamEndReason, SubscriptionStream};
use futures_util::future::{Either, select};
use futures_util::stream::{self, StreamExt};
use tokio::time::Instant;

/// A request extension: the instant a signed subscription's stream must end.
/// Only the envelope layer inserts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignedStreamDeadline(pub Instant);

fn lifetime_over() -> CratestackError {
    StreamEndNotice::new(StreamEndReason::StreamLifetime).into_error()
}

/// `items`, ended with `unavailable` at `deadline`. No deadline: `items`
/// untouched.
pub fn until_stream_deadline<T: Send + 'static>(
    items: SubscriptionStream<T>,
    deadline: Option<SignedStreamDeadline>,
) -> SubscriptionStream<T> {
    let Some(SignedStreamDeadline(at)) = deadline else {
        return items;
    };
    let state = (items, Box::pin(tokio::time::sleep_until(at)), false);
    Box::pin(stream::unfold(
        state,
        |(mut items, mut sleep, ended)| async move {
            if ended {
                return None;
            }
            // The clock first: `select` polls its first argument first, so a
            // source that is always ready cannot starve the deadline.
            match select(sleep.as_mut(), items.next()).await {
                Either::Left(((), _)) => Some((Err(lifetime_over()), (items, sleep, true))),
                Either::Right((Some(item), _)) => {
                    let ended = item.is_err();
                    Some((item, (items, sleep, ended)))
                }
                Either::Right((None, _)) => None,
            }
        },
    ))
}

#[cfg(test)]
mod tests;
