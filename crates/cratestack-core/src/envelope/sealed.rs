//! The sealed message `HmacEnvelope` produces (deprecated, cratestack#1018).

// Defines the deprecated type; see the note in the parent module.
#![allow(deprecated)]

use serde::{Deserialize, Serialize};

use crate::error::CratestackError;

/// The sealed form `HmacEnvelope` produces.
#[deprecated(
    since = "0.19.6",
    note = "use the COSE_Mac0 envelope instead: `cratestack_cose::CoseEnvelope` built with `CoseMode::Mac0` (ADR 0006); removed in the next minor release"
)]
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
    pub(super) fn signing_input(&self) -> Result<Vec<u8>, CratestackError> {
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
            .map_err(|error| CratestackError::Codec(format!("encode envelope body: {error}")))?;
        buf.extend_from_slice(&body_bytes);
        Ok(buf)
    }
}
