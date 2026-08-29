//! Splitting a compact JWT into its header/claims/signature parts (no signature
//! verification — see [`super::verifier::IdTokenVerifier`] for that).

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

use super::{
    claims::{IdTokenClaims, JwtHeader},
    disclosure::split_sd_jwt,
};
use crate::AuthError;

pub(super) fn parse_token_parts(
    token: &str,
) -> Result<(JwtHeader, IdTokenClaims, String, Vec<u8>), AuthError> {
    let mut parts = token.split('.');
    let encoded_header = parts
        .next()
        .ok_or_else(|| AuthError::IdTokenDecoding("missing jwt header".to_string()))?;
    let encoded_payload = parts
        .next()
        .ok_or_else(|| AuthError::IdTokenDecoding("missing jwt payload".to_string()))?;
    let encoded_signature = parts
        .next()
        .ok_or_else(|| AuthError::IdTokenDecoding("missing jwt signature".to_string()))?;
    if parts.next().is_some() {
        return Err(AuthError::IdTokenDecoding(
            "jwt compact form must contain exactly three parts".to_string(),
        ));
    }

    let header: JwtHeader = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(encoded_header)
            .map_err(|error| AuthError::IdTokenDecoding(error.to_string()))?,
    )
    .map_err(|error| AuthError::IdTokenDecoding(error.to_string()))?;
    let claims: IdTokenClaims = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(encoded_payload)
            .map_err(|error| AuthError::IdTokenDecoding(error.to_string()))?,
    )
    .map_err(|error| AuthError::IdTokenDecoding(error.to_string()))?;
    let signature = URL_SAFE_NO_PAD
        .decode(encoded_signature)
        .map_err(|error| AuthError::IdTokenDecoding(error.to_string()))?;

    Ok((
        header,
        claims,
        format!("{encoded_header}.{encoded_payload}"),
        signature,
    ))
}

pub fn decode_id_token_claims_unverified(token: &str) -> Result<IdTokenClaims, AuthError> {
    let (jwt_compact, _) = split_sd_jwt(token);
    let (_, claims, _, _) = parse_token_parts(jwt_compact)?;
    Ok(claims)
}
