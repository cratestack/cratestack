//! Carrying revocations to every replica over the subscription backplane.
//!
//! The payload is JSON text on the server-internal channel
//! [`REVOCATION_CHANNEL`]; it is never part of the client wire. Delivery is
//! at-most-once (`backplane.rs`), so a follower that is told messages may
//! have been lost fails closed by default ([`ResyncPolicy::EndAll`]).

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::StreamExt;

use super::message::Message;
use super::{RevocationKey, StreamRevocations};
use crate::backplane::{BackplaneMessage, SubscriptionBackplane};
use crate::error::{CratestackError, StreamEndReason};

/// The backplane channel revocations travel on.
pub const REVOCATION_CHANNEL: &str = "cratestack.revocation";

/// What a follower does when the backplane says messages may be lost
/// (`Resync`, `Lagged`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ResyncPolicy {
    /// End every watched stream with
    /// [`RevocationResync`](StreamEndReason::RevocationResync). A revocation
    /// may have been lost, so no stream is trusted: fail closed. The default.
    #[default]
    EndAll,
    /// Keep every stream. A lost revocation then goes unnoticed until the
    /// stream's credential expires; choose this only where a flapping
    /// `LISTEN` connection costs more than that window.
    Ignore,
}

impl StreamRevocations {
    /// Revokes `key` here and publishes the revocation to every other
    /// replica. Call it from the logout, suspend or device-revoke handler;
    /// with `PgNotifyBackplane` the same `NOTIFY` can be issued in the
    /// transaction that records the change, so replicas hear of it at
    /// commit. The payload is JSON `{"kind", "key", "reason", "until"}`;
    /// `until` is integer seconds since the epoch (a fraction, as from
    /// `extract(epoch from ...)`, or a numeric string, is accepted and
    /// rounded up; a negative or non-numeric one revokes with no tombstone). Returns how many streams this replica ended. A replica that
    /// is not [following](Self::follow) never hears of it.
    ///
    /// The local revoke happens first and is not undone: when the backplane
    /// refuses the publish this returns `Err`, the streams on this replica
    /// are already ended (and the tombstone kept), the count is lost, and
    /// only the other replicas missed the revocation. Retry the publish.
    pub async fn publish(
        &self,
        backplane: &dyn SubscriptionBackplane,
        key: RevocationKey,
        reason: StreamEndReason,
        until: Option<SystemTime>,
    ) -> Result<usize, CratestackError> {
        let ended = self.revoke(key.clone(), reason, until);
        let (kind, value) = key.parts();
        let payload = serde_json::to_string(&Message {
            kind: kind.to_owned(),
            key: value,
            reason: reason.as_str().to_owned(),
            until: until
                .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
                .map(|elapsed| elapsed.as_secs().into()),
        })
        .map_err(|error| CratestackError::Internal(error.to_string()))?;
        if let Err(error) = backplane.publish(REVOCATION_CHANNEL, &payload).await {
            // The local revoke above stands; only other replicas missed it.
            tracing::warn!(%error, ?reason, "revocation applied locally but not published");
            return Err(error);
        }
        Ok(ended)
    }

    /// [`follow_with`](Self::follow_with) with the default
    /// [`ResyncPolicy::EndAll`]. Spawn the returned future once per
    /// replica: `tokio::spawn(revocations.follow(backplane))`.
    pub fn follow(
        &self,
        backplane: Arc<dyn SubscriptionBackplane>,
    ) -> impl Future<Output = Result<(), CratestackError>> + Send + 'static {
        self.follow_with(backplane, ResyncPolicy::default())
    }

    /// Applies every revocation published on [`REVOCATION_CHANNEL`] to this
    /// registry, for as long as the future is polled.
    ///
    /// A backplane that may have lost messages ends the receiver (`Resync`,
    /// `Lagged`, or the listener dropping): `policy` decides what that does
    /// to the streams already watched, and the follower subscribes again.
    /// Messages published in that gap are the ones `policy` guards against.
    /// It returns only when subscribing fails, after applying `policy`: a
    /// replica that cannot hear revocations must not be trusted, so spawn it
    /// again (with a pause) when it does. However it ends (returning,
    /// panicking, or being dropped or aborted), `policy` is applied last.
    ///
    /// No payload can end the follower. A malformed one is logged and
    /// skipped; one naming a reason this version does not know still
    /// revokes, as [`SessionEnded`](StreamEndReason::SessionEnded); an
    /// `until` too far ahead to represent is clamped to 24 hours from now.
    pub fn follow_with(
        &self,
        backplane: Arc<dyn SubscriptionBackplane>,
        policy: ResyncPolicy,
    ) -> impl Future<Output = Result<(), CratestackError>> + Send + 'static {
        let this = self.clone();
        async move {
            // Fail closed however this future ends: a follower that stops
            // hearing revocations must not leave streams trusted.
            let _fail_closed = ResyncOnDrop {
                registry: this.clone(),
                policy,
            };
            loop {
                let mut receiver = match backplane.subscribe(REVOCATION_CHANNEL).await {
                    Ok(receiver) => receiver,
                    Err(error) => {
                        this.resync(policy);
                        return Err(error);
                    }
                };
                while let Some(message) = receiver.next().await {
                    match message {
                        BackplaneMessage::Payload(payload) => this.apply(&payload),
                        BackplaneMessage::Resync | BackplaneMessage::Lagged => this.resync(policy),
                    }
                }
                // The receiver ended: whatever was published before the next
                // `subscribe` is not heard.
                this.resync(policy);
                yield_now().await;
            }
        }
    }

    pub(super) fn resync(&self, policy: ResyncPolicy) {
        if policy == ResyncPolicy::EndAll {
            self.end_all(StreamEndReason::RevocationResync);
        }
    }
}

struct ResyncOnDrop {
    registry: StreamRevocations,
    policy: ResyncPolicy,
}

impl Drop for ResyncOnDrop {
    fn drop(&mut self) {
        self.registry.resync(self.policy);
    }
}

/// Lets other tasks run once, so a receiver that ends at once, every time,
/// cannot starve the executor (core has no timer to pause with).
async fn yield_now() {
    let mut yielded = false;
    std::future::poll_fn(|cx| {
        if yielded {
            std::task::Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            std::task::Poll::Pending
        }
    })
    .await
}
