//! Typed verification keys.

use std::fmt;

use cratestack_core::CratestackError;
use ed25519_dalek::Signature as EdSignature;
use p256::ecdsa::signature::Verifier;
use subtle::ConstantTimeEq;

use super::hmac::{HmacSecret, hmac_tag};
use crate::alg::CoseAlg;
use crate::thumbprint::{self, KID_LEN};

/// A key a message may be verified with.
///
/// An enum over key *types*, not a bag of bytes, so that the one classic
/// COSE/JOSE mistake is unrepresentable: an Ed25519 public key can never be
/// used as an HMAC secret (an attacker who knows the public key could
/// otherwise MAC anything with it), and a P-256 key can never verify an
/// Ed25519 signature. Verification pairs each variant with the
/// algorithms of its own family and answers `false` for every other pair.
#[derive(Clone, PartialEq, Eq)]
pub enum CoseVerifyKey {
    /// An Ed25519 public key, for `-19`.
    Ed25519(ed25519_dalek::VerifyingKey),
    /// A P-256 public key, for `-9`.
    P256(p256::ecdsa::VerifyingKey),
    /// An HMAC secret, for `4` and `5`.
    Hmac(HmacSecret),
}

impl CoseVerifyKey {
    /// An Ed25519 public key from its 32 bytes. Rejects bytes that are not
    /// a valid point encoding.
    pub fn ed25519(public: &[u8; 32]) -> Result<Self, CratestackError> {
        ed25519_dalek::VerifyingKey::from_bytes(public)
            .map(Self::Ed25519)
            .map_err(|_| CratestackError::Validation("invalid Ed25519 public key".to_owned()))
    }

    /// A P-256 public key from its SEC1 encoding (compressed or not).
    pub fn p256_sec1(sec1: &[u8]) -> Result<Self, CratestackError> {
        p256::ecdsa::VerifyingKey::from_sec1_bytes(sec1)
            .map(Self::P256)
            .map_err(|_| CratestackError::Validation("invalid P-256 public key".to_owned()))
    }

    /// An HMAC secret. Fails below 32 bytes (see [`HmacSecret::new`]).
    pub fn hmac(secret: impl Into<Vec<u8>>) -> Result<Self, CratestackError> {
        HmacSecret::new(secret).map(Self::Hmac)
    }

    /// Whether this key can verify messages of `alg`.
    pub fn supports(&self, alg: CoseAlg) -> bool {
        matches!(
            (self, alg),
            (Self::Ed25519(_), CoseAlg::Ed25519)
                | (Self::P256(_), CoseAlg::Esp256)
                | (Self::Hmac(_), CoseAlg::Hmac256_64 | CoseAlg::Hmac256_256)
        )
    }

    /// The key's RFC 9679 thumbprint.
    pub fn thumbprint(&self) -> [u8; 32] {
        match self {
            Self::Ed25519(key) => thumbprint::okp_ed25519_thumbprint(key.as_bytes()),
            Self::P256(key) => {
                let point = key.to_sec1_point(false);
                let bytes = point.as_bytes();
                let mut x = [0; 32];
                let mut y = [0; 32];
                // Uncompressed SEC1 is `0x04 ‖ x ‖ y`, 65 bytes, for P-256.
                x.copy_from_slice(&bytes[1..33]);
                y.copy_from_slice(&bytes[33..65]);
                thumbprint::ec2_p256_thumbprint(&x, &y)
            }
            Self::Hmac(secret) => thumbprint::symmetric_thumbprint(secret.expose()),
        }
    }

    /// The key's `kid` (§3).
    pub fn kid(&self) -> [u8; KID_LEN] {
        thumbprint::kid_from_thumbprint(&self.thumbprint())
    }

    /// Check `signature` over `to_be_signed` for `alg`. `false` for a key of
    /// another family, a signature of the wrong length, or a bad signature.
    ///
    /// Ed25519 uses `verify_strict`, which rejects small-order keys and
    /// non-canonical signature encodings, so a signature has exactly one
    /// accepted byte form. ESP256 accepts high-`s` signatures: RFC 9053 does
    /// not require low-`s`, and WebCrypto does not produce it. Malleability
    /// cannot replay a message here, because replay is keyed on
    /// `(kid, cti)`, which the signature covers. MAC tags are compared in
    /// constant time.
    pub(crate) fn verify(&self, alg: CoseAlg, to_be_signed: &[u8], signature: &[u8]) -> bool {
        if signature.len() != alg.signature_len() {
            return false;
        }
        match (self, alg) {
            (Self::Ed25519(key), CoseAlg::Ed25519) => EdSignature::from_slice(signature)
                .is_ok_and(|sig| key.verify_strict(to_be_signed, &sig).is_ok()),
            (Self::P256(key), CoseAlg::Esp256) => p256::ecdsa::Signature::from_slice(signature)
                .is_ok_and(|sig| key.verify(to_be_signed, &sig).is_ok()),
            (Self::Hmac(secret), CoseAlg::Hmac256_64 | CoseAlg::Hmac256_256) => {
                let expected = hmac_tag(secret, alg, to_be_signed);
                bool::from(expected.as_slice().ct_eq(signature))
            }
            _ => false,
        }
    }
}

impl fmt::Debug for CoseVerifyKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let family = match self {
            Self::Ed25519(_) => "Ed25519",
            Self::P256(_) => "P256",
            Self::Hmac(_) => "Hmac",
        };
        f.debug_struct("CoseVerifyKey")
            .field("family", &family)
            .field("kid", &self.kid())
            .finish()
    }
}
