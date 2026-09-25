//! `cratestack_auth::NonceStore` -> `cratestack_core::NonceStore`.
//!
//! The two traits are unrelated (ADR 0006, P0 scoping notes on
//! cratestack#1005). Core's, which the envelope's opener calls, asks
//! "record this key until `expires_at`; was it new?". Auth's, which its
//! Redis store implements, is `claim(key_id, nonce, timestamp,
//! replay_window)`: remember `key_id:nonce` until `timestamp +
//! replay_window`, `Err(NonceReused)` if it is already there. The Redis
//! implementation is one atomic `SET key value NX EX ttl`, so two replicas
//! racing on one `cti` cannot both win.

use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use cratestack_auth::{AuthError, NonceStore as AuthNonceStoreTrait, nonce_store_from_redis_url};
use cratestack_core::{CratestackError, NonceStore};

/// The `key_id` every bridged nonce is claimed under.
///
/// The opener passes one string, `cose:<hex kid>:<hex cti>`
/// (`replay::nonce_key`); auth's store takes two and stores
/// `{key_id}:{nonce}` (in Redis, under `cratestack:signature-nonce:`). The
/// bridge does not split the string. It claims all of it as the `nonce`
/// under this fixed `key_id`, so two different opener keys never map to
/// one entry (splitting on `:` would not guarantee that). A COSE entry is
/// therefore `cose-envelope:cose:<hex kid>:<hex cti>`.
///
/// **Shared namespace with signed requests.** Auth's `{key_id}:{nonce}`
/// join is not injective, and `SignedRequestVerifier` claims its nonces
/// the same way, so when both point at one store (the same Redis, or one
/// shared `Arc`) they share a keyspace. A verified signed request whose
/// `keyId` is `cose-envelope` or *starts with* `cose-envelope:` can
/// produce the same entry as a COSE message (for example `keyId = "cose-envelope:cose"`,
/// `nonce = "<hex kid>:<hex cti>"`). The effect is refuse-only: whichever
/// claim comes second is reported as a replay, never accepted. To block a
/// COSE request that way, the signer of such a `keyId` would have to know
/// the victim's random 16-byte `cti` before the victim's request lands.
/// Operators sharing one store between the two must not issue
/// signed-request key ids with the `cose-envelope` prefix.
pub const AUTH_NONCE_KEY_ID: &str = "cose-envelope";

/// `cratestack_auth`'s nonce store (in-memory, or Redis across replicas),
/// as the `cratestack_core::NonceStore` a server [`CoseEnvelope`] records
/// `(kid, cti)` in (§5, `nonce` mode).
///
/// **Expiry.** An entry must outlive `expires_at`, the instant after which
/// the opener would refuse the request as stale anyway (`iat + 2·skew + 1`,
/// see `replay.rs`). Auth's Redis store turns `timestamp + replay_window`
/// into a whole-second TTL against its own clock and rounds **down**, so
/// the bridge claims with `timestamp = now` and `replay_window =
/// (expires_at + 1 s) - now`: the key then lives at least until
/// `expires_at` and at most one second longer. The deadline is computed
/// with checked arithmetic: an `expires_at` too close to
/// `DateTime::<Utc>::MAX_UTC` for `+ 1 s` is `Internal` (a `500`) and no
/// claim is made, because auth's in-memory store adds `timestamp +
/// replay_window` unchecked and would panic. An `expires_at` already in
/// the past is reported new, the same answer core's `InMemoryNonceStore`
/// gives; what is kept depends on the store: Redis floors the TTL at one
/// second, so the key exists for a second, while auth's in-memory store
/// inserts an entry that is already expired and ignores it on the next
/// claim. The opener never asks, since it checks freshness first.
///
/// **Errors.** `NonceReused` is "seen" (`Ok(false)`, the opener's `401`).
/// Every other error means the store could not answer, so it is
/// `CratestackError::Internal` (a `500`, detail kept server-side), never
/// "new". Answering "new" there would make a Redis outage switch replay
/// protection off.
///
/// **Deploying the Redis store.** Replay protection is only as durable as
/// the key. Run the nonce Redis with `maxmemory-policy noeviction`: an
/// evicting policy can drop a live nonce under memory pressure and let its
/// message be replayed. Redis replication is asynchronous, so a failover
/// can lose a `SET` the old primary acknowledged but had not yet
/// replicated, reopening the window for that nonce until it would have
/// expired. Known limitation: auth's Redis store opens a new connection
/// for every claim (cratestack#1070).
///
/// [`CoseEnvelope`]: crate::CoseEnvelope
#[derive(Clone)]
pub struct AuthNonceStore {
    inner: Arc<dyn AuthNonceStoreTrait>,
}

impl AuthNonceStore {
    /// Bridge any `cratestack_auth::NonceStore`.
    pub fn new(inner: Arc<dyn AuthNonceStoreTrait>) -> Self {
        Self { inner }
    }

    /// The Redis-backed store at `redis_url`, via
    /// `cratestack_auth::nonce_store_from_redis_url`.
    ///
    /// Unlike that function, an empty URL is an error, not a silent
    /// in-memory fallback: a multi-replica deployment that lost its Redis
    /// URL would otherwise get one nonce store per replica, and a request
    /// replayed to another replica would be accepted. A deployment that
    /// wants the in-memory store asks for it with [`new`](Self::new).
    ///
    /// The URL is only parsed here; the first connection is made on the
    /// first request, and a failure then is a `500`.
    pub fn redis(redis_url: &str) -> Result<Self, CratestackError> {
        if redis_url.trim().is_empty() {
            return Err(CratestackError::Validation(
                "the COSE nonce store's Redis URL is empty".to_owned(),
            ));
        }
        nonce_store_from_redis_url(Some(redis_url))
            .map(Self::new)
            .map_err(|error| CratestackError::Validation(error.to_string()))
    }
}

impl fmt::Debug for AuthNonceStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthNonceStore").finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl NonceStore for AuthNonceStore {
    async fn record_if_unseen(
        &self,
        nonce: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<bool, CratestackError> {
        let now = Utc::now();
        let replay_window = expires_at
            .checked_add_signed(TimeDelta::seconds(1))
            .map(|deadline| deadline.signed_duration_since(now))
            .ok_or_else(|| CratestackError::Internal("nonce expiry out of range".to_owned()))?;
        match self
            .inner
            .claim(AUTH_NONCE_KEY_ID, nonce, now, replay_window)
            .await
        {
            Ok(()) => Ok(true),
            Err(AuthError::NonceReused) => Ok(false),
            Err(error) => Err(CratestackError::Internal(format!(
                "nonce store failed: {error}"
            ))),
        }
    }
}
