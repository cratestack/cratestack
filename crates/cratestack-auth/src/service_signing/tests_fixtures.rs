#![cfg(test)]
//! Shared fixtures for `service_signing`'s test suite, used by
//! `tests_signing_key`, `tests_verifier`, and `tests_router`.

use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub(super) struct UploadTicketClaims {
    pub(super) iss: String,
    pub(super) sub: String,
    pub(super) iat: i64,
    pub(super) exp: i64,
    pub(super) owner_type: String,
    pub(super) owner_id: String,
    pub(super) purpose: String,
    pub(super) nonce: String,
}

pub(super) fn fixture_signing_key() -> SigningKey {
    // Deterministic test key — same across runs.
    SigningKey::from_bytes(&[7u8; 32])
}

pub(super) fn future_exp() -> i64 {
    chrono::Utc::now().timestamp() + 300
}

pub(super) fn past_exp() -> i64 {
    chrono::Utc::now().timestamp() - 300
}
