//! COSE-signed enrolment challenge responses, and the Ed25519 key used to
//! sign and verify them.

use coset::{CoseSign1, CoseSign1Builder, HeaderBuilder, TaggedCborSerializable, iana};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier};

use crate::error::AuthError;
use crate::id_token::decode_signing_key;
use crate::protocol::EnrollResponse;
use crate::{CHALLENGE_SIGNING_KEY_ENV, ENROLL_CHALLENGE_COSE_KID};

#[cfg(test)]
mod tests;

pub fn build_cose_enroll_response(response: &EnrollResponse) -> Result<Vec<u8>, AuthError> {
    let signing_key = challenge_signing_key()?;
    build_cose_enroll_response_with_key(response, &signing_key)
}

fn build_cose_enroll_response_with_key(
    response: &EnrollResponse,
    signing_key: &SigningKey,
) -> Result<Vec<u8>, AuthError> {
    let payload = minicbor_serde::to_vec(response)
        .map_err(|error| AuthError::ChallengeEncoding(error.to_string()))?;
    let protected = HeaderBuilder::new()
        .algorithm(iana::Algorithm::EdDSA)
        .key_id(ENROLL_CHALLENGE_COSE_KID.as_bytes().to_vec())
        .build();
    let sign1 = CoseSign1Builder::new()
        .protected(protected)
        .payload(payload)
        .create_signature(b"", |to_sign| signing_key.sign(to_sign).to_bytes().to_vec())
        .build();

    sign1
        .to_tagged_vec()
        .map_err(|error| AuthError::ChallengeEncoding(error.to_string()))
}

pub fn parse_cose_enroll_response(bytes: &[u8]) -> Result<EnrollResponse, AuthError> {
    let signing_key = challenge_signing_key()?;
    parse_cose_enroll_response_with_key(bytes, &signing_key)
}

fn parse_cose_enroll_response_with_key(
    bytes: &[u8],
    signing_key: &SigningKey,
) -> Result<EnrollResponse, AuthError> {
    let sign1 = CoseSign1::from_tagged_slice(bytes)
        .map_err(|error| AuthError::ChallengeDecoding(error.to_string()))?;
    let verifying_key = signing_key.verifying_key();

    sign1
        .verify_signature(b"", |signature, data| {
            verify_signature(&verifying_key, signature, data)
        })
        .map_err(|error| AuthError::ChallengeDecoding(error.to_string()))?;

    let payload = sign1.payload.ok_or(AuthError::MissingChallengePayload)?;
    minicbor_serde::from_slice(&payload)
        .map_err(|error| AuthError::ChallengeDecoding(error.to_string()))
}

/// Loads the Ed25519 signing key used to sign and verify COSE enrolment
/// challenge responses (`build_cose_enroll_response` /
/// `parse_cose_enroll_response`) from [`CHALLENGE_SIGNING_KEY_ENV`]
/// (URL-safe-base64-no-pad-encoded 32-byte seed — same encoding as
/// [`ServiceSigningKey::from_env`][crate::ServiceSigningKey::from_env] and
/// [`encode_signing_key`][crate::encode_signing_key]/[`decode_signing_key`]).
///
/// **Security history:** the downstream crate this was absorbed from used
/// to return a hardcoded Ed25519 seed literal here. That seed was committed
/// to that repository's git history and MUST be treated as permanently
/// compromised — anyone with read access to the repo (or its history,
/// forks, or CI logs) can reconstruct it and forge COSE enrolment challenge
/// responses. It must never be reused as a default, fallback, or "example"
/// value anywhere, including in tests — this crate's own tests generate
/// their own key material (see `tests::test_challenge_signing_key_b64`).
///
/// Fails closed: an absent or whitespace-only env var is a hard error, not
/// a silently-generated ephemeral key or a fallback to any default — a
/// service that can't load its challenge signing key must not be able to
/// sign or verify enrolment challenges at all.
fn challenge_signing_key() -> Result<SigningKey, AuthError> {
    challenge_signing_key_from(|name| std::env::var(name))
}

fn challenge_signing_key_from(
    lookup_env: impl Fn(&str) -> Result<String, std::env::VarError>,
) -> Result<SigningKey, AuthError> {
    let raw = lookup_env(CHALLENGE_SIGNING_KEY_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| AuthError::MissingSigningKeyEnv(CHALLENGE_SIGNING_KEY_ENV.to_string()))?;
    decode_signing_key(&raw)
}

fn verify_signature(
    verifying_key: &ed25519_dalek::VerifyingKey,
    signature: &[u8],
    data: &[u8],
) -> Result<(), String> {
    let signature = Signature::from_slice(signature).map_err(|error| error.to_string())?;
    verifying_key
        .verify(data, &signature)
        .map_err(|error| error.to_string())
}
