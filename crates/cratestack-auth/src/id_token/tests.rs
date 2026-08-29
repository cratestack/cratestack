//! Tests for `id_token`, split by concern into sibling submodules to stay
//! under the 200-LoC budget.

mod issuance;
mod jwk;
mod verifier;

use ed25519_dalek::SigningKey;

use super::{issue_id_token, issuer_jwk};
use crate::{AuthError, TokenResponse};

pub(super) const TEST_ISSUER_SIGNING_KID: &str = "issuer-dev-key-1";

pub(super) fn test_issuer_signing_key() -> SigningKey {
    SigningKey::from_bytes(&[
        0x6d, 0x01, 0x97, 0x4a, 0x39, 0x8c, 0x27, 0x7c, 0xc0, 0x2d, 0xb4, 0x51, 0x6d, 0x89, 0xa4,
        0x1f, 0x38, 0x21, 0xb6, 0xde, 0x74, 0xd9, 0x41, 0x20, 0x7a, 0xcf, 0x10, 0x63, 0xf4, 0x9b,
        0x8d, 0x29,
    ])
}

pub(super) fn test_issuer_jwk() -> crate::Jwk {
    issuer_jwk(&test_issuer_signing_key(), TEST_ISSUER_SIGNING_KID)
}

pub(super) fn issue_token_pair(claims: super::IdTokenClaims) -> Result<TokenResponse, AuthError> {
    let id_jwt = issue_id_token(&test_issuer_signing_key(), TEST_ISSUER_SIGNING_KID, &claims)?;
    Ok(TokenResponse {
        token_type: "N_A".to_string(),
        issued_token_type: "urn:ietf:params:oauth:token-type:jwt".to_string(),
        id_jwt,
        expires_in: chrono::Duration::days(365).num_seconds(),
        refresh_token: format!("refresh_{}", cuid2::create_id()),
        cnf: claims.cnf,
    })
}
