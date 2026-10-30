//! Converting between Ed25519 signing/verifying keys and the OKP JWK shape
//! used both for the issuer's published JWKS document and for a holder's
//! bound device key carried in `cnf.jwk`.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};

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

/// The RFC 7638 JWK SHA-256 thumbprint of an OKP key, base64url without
/// padding: the value the principal contract carries as `cnf.jkt` (RFC 9449
/// §6). The hash input is the required members `crv`, `kty` and `x` in that
/// (lexicographic) order, with no whitespace (RFC 8037 §2). `None` for any
/// other key type, or an OKP key missing `crv` or `x`.
pub fn jwk_thumbprint(jwk: &Jwk) -> Option<String> {
    if jwk.kty != "OKP" {
        return None;
    }
    let canonical = serde_json::json!({
        "crv": jwk.crv.as_deref()?,
        "kty": jwk.kty,
        "x": jwk.x.as_deref()?,
    });
    // Written in key order, so the output is canonical whether or not
    // `serde_json`'s `preserve_order` is on; the compact writer adds no
    // whitespace. Serializing a `Value` cannot fail.
    let digest = Sha256::digest(serde_json::to_vec(&canonical).ok()?);
    Some(URL_SAFE_NO_PAD.encode(digest))
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
