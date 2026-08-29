//! Minimal JWT compact-form parser shared by [`super::verifier_verify`].

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::Value;

use crate::AuthError;

use super::minting::JwtHeader;

pub(super) fn parse_jwt_parts(
    token: &str,
) -> Result<(JwtHeader, Value, String, Vec<u8>), AuthError> {
    let mut parts = token.splitn(3, '.');
    let encoded_header = parts
        .next()
        .ok_or_else(|| AuthError::IdTokenDecoding("missing header".to_string()))?;
    let encoded_claims = parts
        .next()
        .ok_or_else(|| AuthError::IdTokenDecoding("missing claims".to_string()))?;
    let encoded_signature = parts
        .next()
        .ok_or_else(|| AuthError::IdTokenDecoding("missing signature".to_string()))?;
    if parts.next().is_some() {
        return Err(AuthError::IdTokenDecoding(
            "unexpected extra segments".to_string(),
        ));
    }

    let header_bytes = URL_SAFE_NO_PAD
        .decode(encoded_header)
        .map_err(|err| AuthError::IdTokenDecoding(err.to_string()))?;
    let header: JwtHeader = serde_json::from_slice(&header_bytes)
        .map_err(|err| AuthError::IdTokenDecoding(err.to_string()))?;

    let claims_bytes = URL_SAFE_NO_PAD
        .decode(encoded_claims)
        .map_err(|err| AuthError::IdTokenDecoding(err.to_string()))?;
    let claims: Value = serde_json::from_slice(&claims_bytes)
        .map_err(|err| AuthError::IdTokenDecoding(err.to_string()))?;

    let signature_bytes = URL_SAFE_NO_PAD
        .decode(encoded_signature)
        .map_err(|err| AuthError::IdTokenDecoding(err.to_string()))?;

    let signing_input = format!("{encoded_header}.{encoded_claims}");
    Ok((header, claims, signing_input, signature_bytes))
}
