//! Ending a live subscription when its principal is revoked, suspended or
//! signed out (cratestack#1381).
//!
//! A stream authenticates once, at open, and keeps its context for every
//! item. Its credential's `exp` bounds it, but nothing else did: a device
//! revoked a minute after the stream opened kept receiving events until the
//! credential ran out. [`StreamRevocations`] is the missing hook: every
//! generated subscription open registers a [`RevocationWatch`] under the
//! keys of its context ([`default_revocation_keys`]), and
//! [`StreamRevocations::revoke`] ends every stream and WebSocket or
//! WebTransport session holding a key, with a typed [`StreamEndNotice`].
//!
//! ```
//! # use cratestack_core::{RevocationKey, StreamEndReason, StreamRevocations};
//! let revocations = StreamRevocations::default();
//! let mut watch = revocations
//!     .watch(vec![RevocationKey::Session("sid-1".into())])
//!     .unwrap();
//! // Logout handler:
//! revocations.revoke(
//!     RevocationKey::Session("sid-1".into()),
//!     StreamEndReason::SessionEnded,
//!     None,
//! );
//! assert_eq!(watch.try_ended().unwrap().reason, StreamEndReason::SessionEnded);
//! ```
//!
//! A *tombstone* (the `until` of a revoke) keeps the key refused for new
//! stream opens until the longest `exp` a still-cached context could carry.
//! It refuses stream opens only: refusing a unary call from a revoked
//! principal stays the job of the app's `AuthProvider`.
//!
//! The registry is sync-only (`tokio::sync`, no timer), so this crate still
//! builds for `wasm32`. Across replicas, [`StreamRevocations::publish`] and
//! [`StreamRevocations::follow`] carry revocations over the subscription
//! backplane.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

use crate::context::CratestackContext;
use crate::error::{StreamEndNotice, StreamEndReason};

mod backplane;
mod keys;
mod message;
mod state;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_backplane;
#[cfg(test)]
mod tests_follower;
#[cfg(test)]
mod tests_keys;
#[cfg(test)]
mod tests_until;
mod watch;

pub use backplane::{REVOCATION_CHANNEL, ResyncPolicy};
pub use keys::{RevocationKey, RevocationKeysFn, default_revocation_keys};
pub use watch::RevocationWatch;

use state::State;

/// The default cap on tombstones kept ([`StreamRevocations::with_max_tombstones`]).
pub const DEFAULT_MAX_TOMBSTONES: usize = 100_000;

/// The registry of live streams by [`RevocationKey`]. Cheap to clone; every
/// clone is the same registry. The default never revokes anything.
#[derive(Clone)]
pub struct StreamRevocations {
    state: Arc<Mutex<State>>,
}

impl Default for StreamRevocations {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(State::new(DEFAULT_MAX_TOMBSTONES))),
        }
    }
}

impl std::fmt::Debug for StreamRevocations {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("StreamRevocations")
    }
}

impl StreamRevocations {
    fn lock(&self) -> MutexGuard<'_, State> {
        // A panic elsewhere cannot leave the maps half-updated: recover.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Bounds the tombstones kept, so a flood of revocations cannot grow
    /// memory without limit (default [`DEFAULT_MAX_TOMBSTONES`]). Over the
    /// cap, the tombstone that lapses soonest is dropped first.
    pub fn with_max_tombstones(self, max: usize) -> Self {
        self.lock().max_tombstones = max;
        self
    }

    /// Names the keys a context's streams are held under (default
    /// [`default_revocation_keys`]). The one place it is set: generated
    /// opens and the WebSocket and WebTransport sessions all read it here,
    /// so a stream and the socket it rides on can never disagree.
    pub fn with_keys(self, keys: RevocationKeysFn) -> Self {
        self.lock().keys = keys;
        self
    }

    /// The keys `ctx` is held under, by [`with_keys`](Self::with_keys).
    pub fn keys_of(&self, ctx: &CratestackContext) -> Vec<RevocationKey> {
        let keys = self.lock().keys;
        keys(ctx)
    }

    /// [`watch`](Self::watch) under [`keys_of`](Self::keys_of) `ctx`.
    pub fn watch_context(
        &self,
        ctx: &CratestackContext,
    ) -> Result<RevocationWatch, StreamEndNotice> {
        self.watch(self.keys_of(ctx))
    }

    /// Replaces the clock tombstones lapse by (tests).
    pub fn with_clock(self, now: fn() -> SystemTime) -> Self {
        self.lock().now = now;
        self
    }

    /// Ends every live stream and socket session holding `key` with
    /// `reason`, and returns how many it ended.
    ///
    /// With `until`, also keeps a tombstone so a later
    /// [`watch`](Self::watch) of `key` is refused until then: set it to the
    /// longest `exp` a context that is still cached could carry. `None`
    /// ends live streams only and keeps no tombstone. Neither does a
    /// `reason` that [`is_unavailable`](StreamEndReason::is_unavailable)
    /// (`stream_lifetime`, `revocation_resync`): clients retry those
    /// without a cap, so a tombstone would loop every client of the key
    /// until it lapsed. Those end live streams only, with a `warn!`.
    pub fn revoke(
        &self,
        key: RevocationKey,
        reason: StreamEndReason,
        until: Option<SystemTime>,
    ) -> usize {
        let mut state = self.lock();
        if let Some(until) = until {
            state.tombstone(key.clone(), reason, until);
        }
        state.end_key(&key, &StreamEndNotice::new(reason))
    }

    /// Ends every watched stream with `reason`; the number ended. Used when
    /// the revocation feed cannot be trusted ([`ResyncPolicy::EndAll`]).
    pub fn end_all(&self, reason: StreamEndReason) -> usize {
        self.lock().end_all(&StreamEndNotice::new(reason))
    }

    /// Registers one stream under `keys`. `Err` when a key is tombstoned:
    /// the open is refused before the client is told it is ready. Dropping
    /// the watch deregisters the stream.
    pub fn watch(&self, keys: Vec<RevocationKey>) -> Result<RevocationWatch, StreamEndNotice> {
        let mut state = self.lock();
        if let Some(reason) = state.tombstoned(&keys) {
            return Err(StreamEndNotice::new(reason));
        }
        let (id, sender) = state.register(keys);
        Ok(RevocationWatch::new(id, self.state.clone(), sender))
    }

    /// How many streams are registered.
    pub fn watching(&self) -> usize {
        self.lock().watches.len()
    }

    /// How many tombstones are held (lapsed ones are pruned lazily).
    pub fn tombstones(&self) -> usize {
        let mut state = self.lock();
        state.prune();
        state.tombstones.len()
    }
}
