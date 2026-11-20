//! The one bound every subscription's stream is wrapped in at open
//! (`docs/design/subscriptions-websocket.md` §6.3, cratestack#1381): it
//! lives no longer than its credential, and ends the moment its principal is
//! revoked, suspended or signed out.
//!
//! The bound sits in the generated open functions, below every binding, so
//! SSE, cbor-seq, WebSocket and WebTransport all get it from one place.
//! What polls first wins: the revocation, then the credential clock, then
//! the items, so an always-ready source starves neither, and a stream that
//! is both revoked and expired reports the stronger reason.

use std::future::pending;
use std::pin::{Pin, pin};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cratestack_core::{
    CratestackContext, CratestackError, RevocationWatch, StreamEndNotice, StreamEndReason,
    StreamRevocations, SubscriptionStream,
};
use futures_util::future::{Either, select};
use futures_util::stream::{self, StreamExt};
use tokio::time::{Instant, Sleep};

/// What bounds one stream's life. `Default` bounds nothing.
#[derive(Debug, Default)]
pub struct StreamBounds {
    /// The credential's `exp`, seconds since the epoch.
    pub expires_at: Option<i64>,
    /// The stream's registration with the principal's keys.
    pub revocation: Option<RevocationWatch>,
}

impl StreamBounds {
    /// The bounds of a stream `ctx` opens: its `exp`, and a watch in
    /// `revocations` under its keys. Refuses (before the client is told the
    /// stream is ready) a principal whose key is tombstoned.
    pub fn open(
        ctx: &CratestackContext,
        revocations: &StreamRevocations,
    ) -> Result<Self, CratestackError> {
        Ok(Self {
            expires_at: ctx.expires_at(),
            revocation: Some(
                revocations
                    .watch_context(ctx)
                    .map_err(StreamEndNotice::into_error)?,
            ),
        })
    }
}

fn deadline_after(expires_at: i64) -> Pin<Box<Sleep>> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0);
    let remaining = Duration::from_secs(expires_at.saturating_sub(now).max(0) as u64);
    Box::pin(tokio::time::sleep_until(Instant::now() + remaining))
}

enum Outcome<T> {
    Revoked(StreamEndNotice),
    Expired,
    Item(Option<Result<T, CratestackError>>),
}

/// `items`, ended with a typed [`CratestackError::StreamEnded`] when
/// `bounds` say so. Already revoked or expired: the first item is that error.
pub fn until_principal_ends<T: Send + 'static>(
    items: SubscriptionStream<T>,
    bounds: StreamBounds,
) -> SubscriptionStream<T> {
    let StreamBounds {
        expires_at,
        revocation,
    } = bounds;
    if expires_at.is_none() && revocation.is_none() {
        return items;
    }
    let state = (items, expires_at.map(deadline_after), revocation, false);
    Box::pin(stream::unfold(
        state,
        |(mut items, mut deadline, mut revocation, ended)| async move {
            if ended {
                return None;
            }
            let outcome = {
                let revoked = async {
                    match revocation.as_mut() {
                        Some(watch) => watch.ended().await,
                        None => pending().await,
                    }
                };
                let expired = async {
                    match deadline.as_mut() {
                        Some(sleep) => sleep.as_mut().await,
                        None => pending().await,
                    }
                };
                let next = items.next();
                match select(pin!(revoked), select(pin!(expired), pin!(next))).await {
                    Either::Left((end, _)) => Outcome::Revoked(end),
                    Either::Right((Either::Left(((), _)), _)) => Outcome::Expired,
                    Either::Right((Either::Right((item, _)), _)) => Outcome::Item(item),
                }
            };
            match outcome {
                Outcome::Revoked(end) => {
                    Some((Err(end.into_error()), (items, deadline, revocation, true)))
                }
                Outcome::Expired => Some((
                    Err(StreamEndNotice::new(StreamEndReason::CredentialExpired).into_error()),
                    (items, deadline, revocation, true),
                )),
                Outcome::Item(Some(item)) => {
                    let ended = item.is_err();
                    Some((item, (items, deadline, revocation, ended)))
                }
                Outcome::Item(None) => None,
            }
        },
    ))
}

#[cfg(test)]
mod tests;
