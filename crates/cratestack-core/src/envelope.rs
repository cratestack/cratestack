//! Signed envelope (HMAC-SHA-256).
//!
//! The contract is intentionally close to COSE_Sign1 with HS256: a
//! content header (kid, alg, timestamp, nonce) is folded into the
//! signing input alongside the body bytes, and the sealed message is
//! a CBOR map `{ kid, alg, ts, nonce, body, mac }`. A full COSE_Sign1
//! implementation with ES256/EdDSA can land later — adding it is
//! non-breaking thanks to the [`keys::KeyProvider`] trait.

mod keys;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::error::CoolError;

pub use keys::{InMemoryNonceStore, KeyProvider, NonceStore, StaticKeyProvider};

/// Maximum tolerable clock skew between sender and receiver when
/// verifying signed envelopes. Banks running cross-region traffic
/// with NTP-sync servers can lower this; off-the-shelf deployments
/// leave it at the default 5 minutes.
const ENVELOPE_DEFAULT_CLOCK_SKEW_SECS: i64 = 300;

/// HMAC-SHA-256 backed envelope. Sealed messages are self-describing
/// CBOR maps: signature recipients can decode the envelope, fetch the
/// key by `kid`, and verify without out-of-band coordination.
#[derive(Clone)]
pub struct HmacEnvelope<K: KeyProvider> {
    keys: Arc<K>,
    signing_kid: String,
    clock_skew_secs: i64,
    nonces: Option<Arc<dyn NonceStore>>,
}

impl<K: KeyProvider> HmacEnvelope<K> {
    pub fn new(keys: Arc<K>, signing_kid: impl Into<String>) -> Self {
        Self {
            keys,
            signing_kid: signing_kid.into(),
            clock_skew_secs: ENVELOPE_DEFAULT_CLOCK_SKEW_SECS,
            nonces: None,
        }
    }

    pub fn with_clock_skew_secs(mut self, secs: i64) -> Self {
        self.clock_skew_secs = secs;
        self
    }

    /// Attach a nonce store so `open` rejects replays. Without this,
    /// the envelope is only protected by the clock-skew window — an
    /// attacker who captured a sealed message can replay it inside
    /// that window.
    pub fn with_nonce_store(mut self, store: Arc<dyn NonceStore>) -> Self {
        self.nonces = Some(store);
        self
    }

    async fn compute_mac(&self, key: &[u8], input: &[u8]) -> Result<Vec<u8>, CoolError> {
        use hmac::{Hmac, Mac};
        let mut mac = <Hmac<sha2::Sha256> as Mac>::new_from_slice(key)
            .map_err(|_| CoolError::Internal("HMAC key length error".to_owned()))?;
        mac.update(input);
        Ok(mac.finalize().into_bytes().to_vec())
    }

    /// Seal a request body. The returned bytes are a CBOR-encoded
    /// [`SealedEnvelope`] payload — the sender wraps these in their
    /// codec of choice on the way out.
    pub async fn seal(&self, payload: serde_json::Value) -> Result<SealedEnvelope, CoolError> {
        let key = self.keys.resolve_signing_key(&self.signing_kid).await?;
        let ts = chrono::Utc::now().timestamp();
        let nonce = uuid::Uuid::new_v4().to_string();
        let mut envelope = SealedEnvelope {
            kid: self.signing_kid.clone(),
            alg: "HS256".to_owned(),
            ts,
            nonce,
            body: payload,
            mac_b64: String::new(),
        };
        let input = envelope.signing_input()?;
        let mac = self.compute_mac(&key, &input).await?;
        use base64::Engine;
        envelope.mac_b64 = base64::engine::general_purpose::STANDARD.encode(mac);
        Ok(envelope)
    }

    /// Verify a sealed envelope. Returns the body on success. Constant-
    /// time MAC compare; clock-skew window enforced; envelope kid is
    /// resolved through the configured provider so callers can rotate
    /// keys without changing the recipient.
    pub async fn open(&self, envelope: &SealedEnvelope) -> Result<serde_json::Value, CoolError> {
        if envelope.alg != "HS256" {
            return Err(CoolError::Unauthorized(format!(
                "unsupported envelope algorithm '{}'",
                envelope.alg,
            )));
        }
        let now = chrono::Utc::now().timestamp();
        let drift = (now - envelope.ts).abs();
        if drift > self.clock_skew_secs {
            return Err(CoolError::Unauthorized(
                "envelope timestamp outside accepted skew window".to_owned(),
            ));
        }
        let key = self.keys.resolve_signing_key(&envelope.kid).await?;
        let input = envelope.signing_input()?;
        let expected = self.compute_mac(&key, &input).await?;
        use base64::Engine;
        let actual = base64::engine::general_purpose::STANDARD
            .decode(&envelope.mac_b64)
            .map_err(|_| CoolError::Unauthorized("envelope MAC is not base64".to_owned()))?;
        if actual.len() != expected.len() {
            return Err(CoolError::Unauthorized(
                "envelope MAC has wrong length".to_owned(),
            ));
        }
        use subtle::ConstantTimeEq;
        if !bool::from(actual.as_slice().ct_eq(expected.as_slice())) {
            return Err(CoolError::Unauthorized(
                "envelope MAC verification failed".to_owned(),
            ));
        }
        if let Some(nonces) = &self.nonces {
            let expires_at = chrono::DateTime::<chrono::Utc>::from_timestamp(
                envelope.ts + self.clock_skew_secs,
                0,
            )
            .ok_or_else(|| CoolError::Unauthorized("envelope timestamp out of range".to_owned()))?;
            let recorded = nonces.record_if_unseen(&envelope.nonce, expires_at).await?;
            if !recorded {
                return Err(CoolError::Unauthorized(
                    "envelope nonce replay detected".to_owned(),
                ));
            }
        }
        Ok(envelope.body.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealedEnvelope {
    pub kid: String,
    pub alg: String,
    pub ts: i64,
    pub nonce: String,
    pub body: serde_json::Value,
    pub mac_b64: String,
}

impl SealedEnvelope {
    pub(crate) fn signing_input(&self) -> Result<Vec<u8>, CoolError> {
        let mut buf = Vec::with_capacity(256);
        buf.extend_from_slice(self.kid.as_bytes());
        buf.push(0);
        buf.extend_from_slice(self.alg.as_bytes());
        buf.push(0);
        buf.extend_from_slice(&self.ts.to_be_bytes());
        buf.push(0);
        buf.extend_from_slice(self.nonce.as_bytes());
        buf.push(0);
        // Body is canonicalised via serde_json::to_vec which uses key-
        // sort order for objects when the input went through
        // `serde_json::Value` — adequate for HMAC integrity (the
        // verifier reconstructs the same bytes the sender signed).
        let body_bytes = serde_json::to_vec(&self.body)
            .map_err(|error| CoolError::Codec(format!("encode envelope body: {error}")))?;
        buf.extend_from_slice(&body_bytes);
        Ok(buf)
    }
}
