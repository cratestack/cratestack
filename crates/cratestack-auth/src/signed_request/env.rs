//! Parsing of the `CRATESTACK_AUTH_SIGNATURE_*` env vars used by
//! [`super::SignedRequestVerifier::from_env`].

use std::collections::HashMap;
use std::env;

use ed25519_dalek::VerifyingKey;

use super::consts::{SIGNATURE_TRUSTED_ISSUERS_ENV, SIGNATURE_TRUSTED_KEYS_ENV};
use super::keys::decode_verifying_key;
use crate::AuthError;

/// Returns an empty Vec when the env var is unset rather than
/// erroring. Used by `from_env` so a service that wires JWKS-only
/// doesn't have to also set a stub static-keys env var.
pub(super) fn parse_trusted_keys_from_env_optional()
-> Result<Vec<(String, VerifyingKey)>, AuthError> {
    match env::var(SIGNATURE_TRUSTED_KEYS_ENV) {
        Ok(raw) if !raw.trim().is_empty() => parse_trusted_keys(&raw),
        _ => Ok(Vec::new()),
    }
}

/// Parse `CRATESTACK_AUTH_SIGNATURE_TRUSTED_ISSUERS` as a JSON object mapping
/// issuer name → JWKS URL. The verifier walks every entry on cache
/// miss to find the kid carried in the signed-request header.
///
/// Example:
///
/// ```text
/// CRATESTACK_AUTH_SIGNATURE_TRUSTED_ISSUERS={
///   "vendor-service": "http://vendor-service:8082/jwks.json",
///   "order-service":  "http://order-service:8084/jwks.json"
/// }
/// ```
///
/// Empty / unset returns an empty map (callers decide whether
/// JWKS-only is acceptable for them).
pub(super) fn parse_trusted_issuers_from_env_optional() -> Result<HashMap<String, String>, AuthError>
{
    let raw = match env::var(SIGNATURE_TRUSTED_ISSUERS_ENV) {
        Ok(raw) if !raw.trim().is_empty() => raw,
        _ => return Ok(HashMap::new()),
    };
    let parsed: HashMap<String, String> = serde_json::from_str(&raw).map_err(|err| {
        AuthError::InvalidTrustedSigningKeys(format!(
            "{SIGNATURE_TRUSTED_ISSUERS_ENV} must be a JSON object: {err}",
        ))
    })?;
    Ok(parsed)
}

fn parse_trusted_keys(raw: &str) -> Result<Vec<(String, VerifyingKey)>, AuthError> {
    let mut trusted_keys = Vec::new();

    for entry in raw
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        let (key_id, encoded_key) = entry.split_once(':').ok_or_else(|| {
            AuthError::InvalidTrustedSigningKeys(
                "expected CRATESTACK_AUTH_SIGNATURE_TRUSTED_KEYS entries in keyId:base64url format"
                    .to_string(),
            )
        })?;

        let verifying_key = decode_verifying_key(encoded_key)?;
        trusted_keys.push((key_id.to_string(), verifying_key));
    }

    if trusted_keys.is_empty() {
        return Err(AuthError::MissingTrustedSigningKeys);
    }

    Ok(trusted_keys)
}

pub(super) fn parse_window_seconds(env_name: &str, default_value: i64) -> Result<i64, AuthError> {
    match env::var(env_name) {
        Ok(value) => {
            let parsed = value.parse::<i64>().map_err(|error| {
                AuthError::InvalidTrustedSigningKeys(format!(
                    "{env_name} must be an integer: {error}"
                ))
            })?;
            if parsed <= 0 {
                return Err(AuthError::InvalidTrustedSigningKeys(format!(
                    "{env_name} must be greater than zero"
                )));
            }
            Ok(parsed)
        }
        Err(_) => Ok(default_value),
    }
}
