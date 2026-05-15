//! Key-resolution and nonce-replay-tracking traits used by the signed
//! envelope. Both are async traits so production deployments can plug
//! Vault / KMS / HSM (`KeyProvider`) and Redis (`NonceStore`) without
//! changing the envelope code.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use crate::error::CoolError;

/// Resolves signing keys by kid (key id). Banks running multi-tenant
/// or rotating keysets implement this so the envelope code never has
/// to know the storage mechanism. Implementations must be constant-
/// time for not-found vs wrong-tenant errors — never use the error
/// message to leak whether a key id exists.
#[async_trait::async_trait]
pub trait KeyProvider: Send + Sync + 'static {
    /// Return the raw key bytes for the given `kid`. For HMAC this is
    /// the symmetric secret. Error if the key is unknown.
    async fn resolve_signing_key(&self, kid: &str) -> Result<Vec<u8>, CoolError>;
}

/// In-memory [`KeyProvider`] for tests and single-tenant deployments.
/// Banks running real workloads bring a backed implementation (KMS,
/// Vault, HSM).
#[derive(Debug, Clone, Default)]
pub struct StaticKeyProvider {
    keys: BTreeMap<String, Vec<u8>>,
}

impl StaticKeyProvider {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_key(mut self, kid: impl Into<String>, key: Vec<u8>) -> Self {
        self.keys.insert(kid.into(), key);
        self
    }
}

#[async_trait::async_trait]
impl KeyProvider for StaticKeyProvider {
    async fn resolve_signing_key(&self, kid: &str) -> Result<Vec<u8>, CoolError> {
        self.keys
            .get(kid)
            .cloned()
            .ok_or_else(|| CoolError::Unauthorized("unknown signing key".to_owned()))
    }
}

/// Tracks the nonces of sealed envelopes that have already been
/// verified inside the clock-skew window, so a captured-and-replayed
/// request gets rejected the second time. Banks running multi-replica
/// deployments back this with Redis so the rejection holds cluster-wide.
#[async_trait::async_trait]
pub trait NonceStore: Send + Sync + 'static {
    /// Attempt to register `nonce` as seen. Returns `Ok(true)` if it
    /// is the first time we see it (caller may proceed); `Ok(false)`
    /// if it was already recorded (caller should reject). Implementations
    /// must drop entries past `expires_at` to keep the working set bounded.
    async fn record_if_unseen(
        &self,
        nonce: &str,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, CoolError>;
}

/// In-memory nonce store. One mutex; the working set is bounded by
/// the clock-skew window — a 5-minute skew at 10k req/s caps at ~3M
/// entries, which is fine. Production multi-replica deployments swap
/// in Redis.
#[derive(Debug, Clone, Default)]
pub struct InMemoryNonceStore {
    seen: Arc<RwLock<BTreeMap<String, chrono::DateTime<chrono::Utc>>>>,
}

impl InMemoryNonceStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait::async_trait]
impl NonceStore for InMemoryNonceStore {
    async fn record_if_unseen(
        &self,
        nonce: &str,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, CoolError> {
        let mut seen = self
            .seen
            .write()
            .map_err(|_| CoolError::Internal("nonce store poisoned".to_owned()))?;
        let now = chrono::Utc::now();
        seen.retain(|_, exp| *exp > now);
        if seen.contains_key(nonce) {
            return Ok(false);
        }
        seen.insert(nonce.to_owned(), expires_at);
        Ok(true)
    }
}
