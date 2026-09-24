//! In-process COSE_Sign1 signers. A key held in a KMS or an HSM implements
//! [`CoseSigner`] itself instead.

use std::fmt;

use cratestack_core::CratestackError;
// One trait for both key types: `ed25519-dalek` 3 and `p256` 0.14 both
// re-export `signature` 3's `Signer`.
use ed25519_dalek::Signer as _;
use p256::ecdsa::signature::DigestSigner as _;
use sha2::{Digest, Sha256};

use super::traits::CoseSigner;
use super::verify_key::CoseVerifyKey;
use crate::alg::CoseAlg;
use crate::thumbprint::KID_LEN;

/// An Ed25519 (`-19`) signer. Ed25519 is deterministic, so the shared
/// vectors compare its output byte for byte.
///
/// It does not override [`CoseSigner::sign_chunks`]: PureEdDSA hashes the
/// message twice (for the nonce, then for the challenge), and
/// `ed25519-dalek`'s safe signing API takes it as one slice, so the envelope
/// builds the to-be-signed structure contiguously for this signer, which
/// copies the payload once. `ed25519-dalek` 3 does offer a two-pass
/// streaming primitive, but only in its `hazmat` module, which is not used
/// here.
#[derive(Clone)]
pub struct Ed25519Signer {
    key: ed25519_dalek::SigningKey,
    kid: [u8; KID_LEN],
}

impl Ed25519Signer {
    /// The signer for the RFC 8032 secret key `seed` (32 random bytes). The
    /// key is expanded here, and wiped when the signer is dropped.
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let key = ed25519_dalek::SigningKey::from_bytes(seed);
        let kid = CoseVerifyKey::from_ed25519(key.verifying_key()).kid();
        Self { key, kid }
    }

    /// The matching verification key, for a resolver.
    pub fn verify_key(&self) -> CoseVerifyKey {
        CoseVerifyKey::from_ed25519(self.key.verifying_key())
    }
}

impl fmt::Debug for Ed25519Signer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ed25519Signer")
            .field("kid", &self.kid)
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl CoseSigner for Ed25519Signer {
    fn alg(&self) -> CoseAlg {
        CoseAlg::Ed25519
    }

    fn kid(&self) -> &[u8] {
        &self.kid
    }

    async fn sign(&self, to_be_signed: &[u8]) -> Result<Vec<u8>, CratestackError> {
        Ok(self.key.sign(to_be_signed).to_bytes().to_vec())
    }
}

/// An ESP256 (`-9`) signer from a 32-byte private scalar.
///
/// Signatures are deterministic (RFC 6979), so this signer's output is
/// reproducible. What it returns may have a high `s`: the envelope
/// normalises every ESP256 signature to low-`s` after the signer returns,
/// whichever signer it was (this one, a KMS, WebCrypto), because the
/// verifier accepts low-`s` only. ECDSA verifiers do not care how `k` was
/// chosen, so a randomized signature from a KMS verifies the same way once
/// normalised.
#[derive(Clone)]
pub struct P256Signer {
    key: p256::ecdsa::SigningKey,
    kid: [u8; KID_LEN],
}

impl P256Signer {
    /// Fails with `CratestackError::Validation` for a scalar that is zero
    /// or not below the group order.
    pub fn from_scalar(scalar: &[u8; 32]) -> Result<Self, CratestackError> {
        let key = p256::ecdsa::SigningKey::from_slice(scalar)
            .map_err(|_| CratestackError::Validation("invalid P-256 private key".to_owned()))?;
        let kid = CoseVerifyKey::from_p256(*key.verifying_key()).kid();
        Ok(Self { key, kid })
    }

    /// The matching verification key, for a resolver.
    pub fn verify_key(&self) -> CoseVerifyKey {
        CoseVerifyKey::from_p256(*self.key.verifying_key())
    }

    fn sign_with(
        &self,
        feed: impl Fn(&mut Sha256),
    ) -> Result<p256::ecdsa::Signature, CratestackError> {
        self.key
            .try_sign_digest(|digest: &mut Sha256| {
                feed(digest);
                Ok(())
            })
            .map_err(|_| CratestackError::Internal("P-256 signing failed".to_owned()))
    }
}

impl fmt::Debug for P256Signer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("P256Signer")
            .field("kid", &self.kid)
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl CoseSigner for P256Signer {
    fn alg(&self) -> CoseAlg {
        CoseAlg::Esp256
    }

    fn kid(&self) -> &[u8] {
        &self.kid
    }

    async fn sign(&self, to_be_signed: &[u8]) -> Result<Vec<u8>, CratestackError> {
        let signature = self.sign_with(|digest| Digest::update(digest, to_be_signed))?;
        Ok(signature.to_bytes().to_vec())
    }

    /// ESP256 signs SHA-256 of the message, so the structure is hashed
    /// piece by piece. The result is the one [`sign`](Self::sign) gives for
    /// the concatenation (RFC 6979 derives `k` from the same digest).
    fn sign_chunks(&self, to_be_signed: &[&[u8]]) -> Option<Result<Vec<u8>, CratestackError>> {
        Some(
            self.sign_with(|digest| {
                for chunk in to_be_signed {
                    Digest::update(digest, chunk);
                }
            })
            .map(|signature| signature.to_bytes().to_vec()),
        )
    }
}
