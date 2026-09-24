//! In-process COSE_Sign1 signers. A key held in a KMS or an HSM implements
//! [`CoseSigner`] itself instead.

use std::fmt;

use cratestack_core::CratestackError;
// One trait for both key types: `ed25519-dalek` 3 and `p256` 0.14 both
// re-export `signature` 3's `Signer`.
use ed25519_dalek::Signer as _;

use super::traits::CoseSigner;
use super::verify_key::CoseVerifyKey;
use crate::alg::CoseAlg;
use crate::thumbprint::KID_LEN;

/// An Ed25519 (`-19`) signer from a 32-byte seed. Ed25519 is deterministic,
/// so the shared vectors compare its output byte for byte.
#[derive(Clone)]
pub struct Ed25519Signer {
    key: ed25519_dalek::SigningKey,
    kid: [u8; KID_LEN],
}

impl Ed25519Signer {
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let key = ed25519_dalek::SigningKey::from_bytes(seed);
        let kid = CoseVerifyKey::Ed25519(key.verifying_key()).kid();
        Self { key, kid }
    }

    /// The matching verification key, for a resolver.
    pub fn verify_key(&self) -> CoseVerifyKey {
        CoseVerifyKey::Ed25519(self.key.verifying_key())
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
/// reproducible and the shared vectors compare it byte for byte. ECDSA
/// verifiers do not care how `k` was chosen, so a randomized signature
/// from a KMS or from WebCrypto verifies the same way.
#[derive(Clone)]
pub struct P256Signer {
    key: p256::ecdsa::SigningKey,
    kid: [u8; KID_LEN],
}

impl P256Signer {
    /// Fails for a scalar that is zero or not below the group order.
    pub fn from_scalar(scalar: &[u8; 32]) -> Result<Self, CratestackError> {
        let key = p256::ecdsa::SigningKey::from_slice(scalar)
            .map_err(|_| CratestackError::Validation("invalid P-256 private key".to_owned()))?;
        let kid = CoseVerifyKey::P256(*key.verifying_key()).kid();
        Ok(Self { key, kid })
    }

    /// The matching verification key, for a resolver.
    pub fn verify_key(&self) -> CoseVerifyKey {
        CoseVerifyKey::P256(*self.key.verifying_key())
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
        let signature: p256::ecdsa::Signature = self
            .key
            .try_sign(to_be_signed)
            .map_err(|_| CratestackError::Internal("P-256 signing failed".to_owned()))?;
        Ok(signature.to_bytes().to_vec())
    }
}
