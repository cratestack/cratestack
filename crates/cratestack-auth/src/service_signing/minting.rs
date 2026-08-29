//! Generic Ed25519 JWT minter shared by [`super::signing_key`] and the
//! verifier's tests.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};

use crate::AuthError;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct JwtHeader {
    pub(super) alg: String,
    typ: String,
    pub(super) kid: String,
}

/// Mint a compact-form Ed25519 JWT for arbitrary serializable
/// claims. The header is fixed to `{alg: "EdDSA", typ: "JWT", kid}`;
/// the consumer's job is to populate envelope claims (`iss`, `iat`,
/// `exp`, `sub`, ...) inside `C`.
///
/// Use [`super::ServiceSigningKey::mint`] in service code; this free
/// function is exposed for tests and for callers that hold a raw
/// [`SigningKey`].
pub fn mint_signed_token<C: Serialize>(
    signing_key: &SigningKey,
    kid: &str,
    claims: &C,
) -> Result<String, AuthError> {
    let header = JwtHeader {
        alg: "EdDSA".to_string(),
        typ: "JWT".to_string(),
        kid: kid.to_string(),
    };
    let encoded_header = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&header).map_err(|err| AuthError::IdTokenEncoding(err.to_string()))?,
    );
    let encoded_claims = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(claims).map_err(|err| AuthError::IdTokenEncoding(err.to_string()))?,
    );
    let signing_input = format!("{encoded_header}.{encoded_claims}");
    let signature = signing_key.sign(signing_input.as_bytes());
    Ok(format!(
        "{signing_input}.{}",
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    ))
}
