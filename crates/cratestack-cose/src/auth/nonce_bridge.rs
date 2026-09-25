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
/// `cratestack:signature-nonce:<key_id>:<nonce>`. The bridge does not
/// split the string. It claims all of it as the `nonce` under this fixed
/// `key_id`, which keeps the mapping injective for any string (splitting
/// on `:` is not). So a COSE entry in Redis is
/// `cratestack:signature-nonce:cose-envelope:cose:<hex kid>:<hex cti>`.
/// It can only coincide with a signed-request entry whose verified `keyId`
/// is `cose-envelope` and whose `nonce` is `cose:<kid>:<cti>`, and even
/// then the effect is a refused request, not an accepted one.
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
/// expires_at - now + 1 s`: the key then lives at least until `expires_at`
/// and at most one second longer. An `expires_at` already in the past is
/// still recorded (for at least one second, auth's floor) and reported new,
/// the same answer core's `InMemoryNonceStore` gives; the opener never
/// asks, since it checks freshness first.
///
/// **Errors.** `NonceReused` is "seen" (`Ok(false)`, the opener's `401`).
/// Every other error means the store could not answer, so it is
/// `CratestackError::Internal` (a `500`, detail kept server-side), never
/// "new". Answering "new" there would make a Redis outage switch replay
/// protection off.
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
            .signed_duration_since(now)
            .checked_add(&TimeDelta::seconds(1))
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
