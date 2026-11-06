//! Durable inner calls. Storage and commit-before-send ordering belong to the caller.

use serde::{Deserialize, Serialize};

use crate::{ClientError, RuntimeHeader};

/// A prepared unary mutation, including its final inner payload and key mode.
///
/// Persist [`Self::to_bytes`] before the first `execute_prepared` attempt. Restore
/// with [`Self::from_bytes`], then execute with the same service, codec, sealing
/// mode and op contract. A changed contract is refused: this format is not an
/// old-schema migration mechanism. Each sealed attempt gets fresh COSE claims;
/// this object contains neither an outer seal nor authentication credentials.
/// Protect journal contents like the business payloads they contain.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedCall {
    pub(super) version: u8,
    pub(super) base_url: String,
    pub(super) method: String,
    pub(super) path: String,
    pub(super) route: String,
    pub(super) params: Vec<String>,
    pub(super) query: Option<String>,
    pub(super) headers: Vec<RuntimeHeader>,
    pub(super) body: Vec<u8>,
    pub(super) content_type: String,
    pub(super) plain_type: String,
    pub(super) accept: String,
    pub(super) op: String,
    pub(super) contract: [u8; 32],
    pub(super) key_tables: Vec<u8>,
    pub(super) sealed: bool,
}

pub(super) const MAX_SERIALIZED: usize = 16 * 1024 * 1024;
pub(super) const MAX_PAYLOAD: usize = 8 * 1024 * 1024;

impl PreparedCall {
    /// Serialize a validated descriptor; no request has to be sent first.
    pub fn to_bytes(&self) -> Result<Vec<u8>, ClientError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|e| bad(e.to_string()))?;
        if bytes.len() > MAX_SERIALIZED {
            return Err(bad("prepared descriptor exceeds 16 MiB"));
        }
        Ok(bytes)
    }

    /// Restore a bounded, versioned descriptor. Execution validates its runtime
    /// binding again, including when a caller uses serde directly instead.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ClientError> {
        if bytes.len() > MAX_SERIALIZED {
            return Err(bad("prepared descriptor exceeds 16 MiB"));
        }
        let call: Self = serde_json::from_slice(bytes).map_err(|e| bad(e.to_string()))?;
        call.validate()?;
        Ok(call)
    }

    /// Final inner bytes; these are preserved across every attempt.
    pub fn payload(&self) -> &[u8] {
        &self.body
    }
    /// The original inner media type, including compact mode when negotiated.
    pub fn payload_type(&self) -> &str {
        &self.content_type
    }
    /// The contract under which the payload was prepared.
    pub fn contract_sha(&self) -> &[u8; 32] {
        &self.contract
    }
}

pub(super) fn bad(message: impl Into<String>) -> ClientError {
    ClientError::BadInput(message.into())
}
