//! Converting between Ed25519 signing/verifying keys and the OKP JWK shape
//! used both for the issuer's published JWKS document and for a holder's
//! bound device key carried in `cnf.jwk`.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{SigningKey, VerifyingKey};

use crate::{AuthError, Jwk, decode_verifying_key};

pub fn issuer_jwk(signing_key: &SigningKey, kid: &str) -> Jwk {
    let verifying_key = signing_key.verifying_key();
    Jwk {
        kty: "OKP".to_string(),
        kid: kid.to_string(),
        alg: "EdDSA".to_string(),
        key_use: "sig".to_string(),
        crv: Some("Ed25519".to_string()),
        x: Some(URL_SAFE_NO_PAD.encode(verifying_key.as_bytes())),
    }
}

/// Build an OKP/Ed25519 JWK for a verifying (public) key. Used to embed a
/// holder's device key in an id_jwt `cnf.jwk` so JWKS-verifying services can
/// check device-signed requests without their own device-key registry.
pub fn verifying_key_jwk(verifying_key: &VerifyingKey, kid: &str) -> Jwk {
    Jwk {
        kty: "OKP".to_string(),
        kid: kid.to_string(),
        alg: "EdDSA".to_string(),
        key_use: "sig".to_string(),
        crv: Some("Ed25519".to_string()),
        x: Some(URL_SAFE_NO_PAD.encode(verifying_key.as_bytes())),
    }
}

/// Recover the Ed25519 public key from an OKP JWK (the inverse of
/// [`verifying_key_jwk`]). Rejects non-OKP / non-Ed25519 JWKs and a missing `x`.
pub fn verifying_key_from_jwk(jwk: &Jwk) -> Result<VerifyingKey, AuthError> {
    if jwk.kty != "OKP" {
        return Err(AuthError::InvalidPublicKey(format!(
            "unsupported cnf jwk kty: {}",
            jwk.kty
        )));
    }
    if jwk.crv.as_deref() != Some("Ed25519") {
        return Err(AuthError::InvalidPublicKey(format!(
            "unsupported cnf jwk crv: {:?}",
            jwk.crv
        )));
    }
    let x = jwk
        .x
        .as_deref()
        .ok_or_else(|| AuthError::InvalidPublicKey("cnf jwk missing x".to_string()))?;
    decode_verifying_key(x)
}

pub fn encode_signing_key(signing_key: &SigningKey) -> String {
    URL_SAFE_NO_PAD.encode(signing_key.to_bytes())
}

pub fn decode_signing_key(encoded: &str) -> Result<SigningKey, AuthError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|error| AuthError::IdTokenDecoding(error.to_string()))?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| AuthError::IdTokenDecoding("invalid signing key length".to_string()))?;
    Ok(SigningKey::from_bytes(&bytes))
}
