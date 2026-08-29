//! [`MultiIssuerJwksVerifier::verify`] — the signature/expiry/trust check.
//!
//! JWKS fetch + per-`kid` cache lookup this depends on lives in
//! [`super::verifier_cache`].

use ed25519_dalek::{Signature, Verifier};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::AuthError;

use super::jwt_parse::parse_jwt_parts;
use super::verifier_types::{IssuerEntry, MultiIssuerJwksVerifier, VerifiedToken};

/// Minimal envelope claims any signed JWT issued via this kit must
/// carry. Used only for the verifier's pre-deserialisation peek;
/// the consumer's own claim struct can include additional fields.
#[derive(Clone, Debug, Deserialize)]
struct EnvelopeClaims {
    iss: String,
    exp: i64,
    #[serde(default)]
    iat: Option<i64>,
}

impl MultiIssuerJwksVerifier {
    /// Verify a compact-form JWT. Checks signature against the
    /// issuer's JWKS, that `iss` is in the trust list, and that
    /// `exp` is in the future. Claim-specific validation (audience,
    /// scope, nonce, ...) is the consumer's job.
    pub async fn verify<C: DeserializeOwned>(
        &self,
        token: &str,
    ) -> Result<VerifiedToken<C>, AuthError> {
        let (header, claims_value, signing_input, signature_bytes) = parse_jwt_parts(token)?;
        if header.alg != "EdDSA" {
            return Err(AuthError::UnsupportedIdTokenAlgorithm(header.alg));
        }

        let envelope: EnvelopeClaims = serde_json::from_value(claims_value.clone())
            .map_err(|err| AuthError::IdTokenDecoding(err.to_string()))?;

        let issuer_entry: &IssuerEntry = self
            .inner
            .issuers
            .get(&envelope.iss)
            .ok_or_else(|| AuthError::UntrustedIssuer(envelope.iss.clone()))?;

        if envelope.exp <= chrono::Utc::now().timestamp() {
            return Err(AuthError::IdTokenExpired);
        }

        let verifying_key = self
            .verifying_key_for(&envelope.iss, issuer_entry, &header.kid)
            .await?;
        let signature = Signature::from_slice(&signature_bytes)
            .map_err(|err| AuthError::IdTokenDecoding(err.to_string()))?;
        verifying_key
            .verify(signing_input.as_bytes(), &signature)
            .map_err(|_| AuthError::IdTokenVerificationFailed)?;

        let claims: C = serde_json::from_value(claims_value)
            .map_err(|err| AuthError::IdTokenDecoding(err.to_string()))?;

        Ok(VerifiedToken {
            issuer: envelope.iss,
            kid: header.kid,
            expires_at: envelope.exp,
            issued_at: envelope.iat,
            claims,
        })
    }
}
