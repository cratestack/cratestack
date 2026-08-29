//! Base64url encoding/decoding for verifying keys and Ed25519 signatures.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signature, VerifyingKey};

use crate::AuthError;

pub fn encode_verifying_key(verifying_key: &VerifyingKey) -> String {
    URL_SAFE_NO_PAD.encode(verifying_key.as_bytes())
}

pub fn decode_verifying_key(encoded: &str) -> Result<VerifyingKey, AuthError> {
    let bytes =
        decode_url_safe(encoded).map_err(|error| AuthError::InvalidPublicKey(error.to_string()))?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| AuthError::InvalidPublicKey("expected 32-byte ed25519 key".to_string()))?;

    VerifyingKey::from_bytes(&bytes).map_err(|error| AuthError::InvalidPublicKey(error.to_string()))
}

pub(super) fn decode_signature(encoded: &str) -> Result<Signature, AuthError> {
    decode_signature_url_safe(encoded)
}

/// Decodes a base64url-encoded Ed25519 signature. Tolerates both padded and
/// unpadded forms. Public so other services (e.g. auth-service's device-
/// pairing flow) can verify ad-hoc signatures without re-implementing the
/// decode dance.
pub fn decode_signature_url_safe(encoded: &str) -> Result<Signature, AuthError> {
    let bytes = decode_url_safe(encoded)
        .map_err(|error| AuthError::InvalidSignatureEncoding(error.to_string()))?;
    Signature::from_slice(&bytes)
        .map_err(|error| AuthError::InvalidSignatureEncoding(error.to_string()))
}

fn decode_url_safe(encoded: &str) -> Result<Vec<u8>, base64::DecodeError> {
    URL_SAFE_NO_PAD
        .decode(encoded)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(encoded))
}
