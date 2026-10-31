//! Compact JWS and hashing helpers the device-signature and DPoP fixtures
//! share. They sign with real keys; nothing is mocked.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Base64url without padding.
pub fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Lower-case hex SHA-256, as `qsh` and `bdh` carry it.
pub fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// `header.payload.signature`; `sign` returns the base64url signature of the
/// signing input.
pub fn compact(header: &Value, claims: &Value, sign: impl Fn(&[u8]) -> String) -> String {
    let header = b64(&serde_json::to_vec(header).expect("a header"));
    let claims = b64(&serde_json::to_vec(claims).expect("claims"));
    let input = format!("{header}.{claims}");
    let signature = sign(input.as_bytes());
    format!("{input}.{signature}")
}

/// Seconds since the epoch.
pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
