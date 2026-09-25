//! Verification keys.

use std::fmt;

use cratestack_core::CratestackError;

use super::hmac::{HmacSecret, is_mac};
use crate::alg::CoseAlg;
use crate::thumbprint::{self, KID_LEN};

/// A key a message may be verified with, bound to exactly one algorithm.
///
/// Opaque: built from bytes ([`ed25519`](Self::ed25519),
/// [`p256_sec1`](Self::p256_sec1), [`hmac`](Self::hmac),
/// [`hmac_from_secret`](Self::hmac_from_secret)) and read back as
/// bytes ([`ed25519_bytes`](Self::ed25519_bytes),
/// [`p256_sec1_uncompressed`](Self::p256_sec1_uncompressed)), so no
/// `ed25519-dalek` or `p256` type is part of this crate's public API and
/// either can be upgraded without a breaking change.
///
/// Internally an enum over key *types*, not a bag of bytes, so the classic
/// COSE/JOSE mistake is unrepresentable: an Ed25519 public key can never be
/// used as an HMAC secret (an attacker who knows the public key could
/// otherwise MAC anything with it), and a P-256 key can never verify an
/// Ed25519 signature.
///
/// **One key, one algorithm.** An HMAC key carries the algorithm it was
/// configured for, and [`supports`](Self::supports) matches it exactly:
/// a secret deployed for HMAC 256/256 never accepts a 64-bit HMAC 256/64
/// tag, which a sender could otherwise pick to cut a forger's work from
/// 2²⁵⁶ to 2⁶⁴ online attempts. (The two share a `kid`: RFC 9679's
/// symmetric thumbprint covers only the secret. A deployment that wants
/// both lists the secret twice, once per algorithm.)
///
/// The RFC 9679 thumbprint is computed once, at construction.
#[derive(Clone, PartialEq, Eq)]
pub struct CoseVerifyKey {
    pub(super) repr: Repr,
    thumbprint: [u8; 32],
}

/// The key material. Visible to `verify.rs`, nothing else.
#[derive(Clone, PartialEq, Eq)]
pub(super) enum Repr {
    Ed25519(ed25519_dalek::VerifyingKey),
    P256(p256::ecdsa::VerifyingKey),
    Hmac { alg: CoseAlg, secret: HmacSecret },
}

impl CoseVerifyKey {
    /// An Ed25519 (`-19`) public key from its 32 bytes. Fails with
    /// `CratestackError::Validation` for bytes that are not a point
    /// encoding.
    pub fn ed25519(public: &[u8; 32]) -> Result<Self, CratestackError> {
        ed25519_dalek::VerifyingKey::from_bytes(public)
            .map(Self::from_ed25519)
            .map_err(|_| CratestackError::Validation("invalid Ed25519 public key".to_owned()))
    }

    /// A P-256 (`-9`) public key from its SEC1 encoding (compressed or
    /// not). Fails with `CratestackError::Validation` for a point not on the
    /// curve.
    pub fn p256_sec1(sec1: &[u8]) -> Result<Self, CratestackError> {
        p256::ecdsa::VerifyingKey::from_sec1_bytes(sec1)
            .map(Self::from_p256)
            .map_err(|_| CratestackError::Validation("invalid P-256 public key".to_owned()))
    }

    /// An HMAC secret for exactly `alg` ([`CoseAlg::Hmac256_64`] or
    /// [`CoseAlg::Hmac256_256`]). Fails with `CratestackError::Validation`
    /// for another algorithm or a secret below 32 bytes (see
    /// [`HmacSecret`]).
    pub fn hmac(alg: CoseAlg, secret: impl Into<Vec<u8>>) -> Result<Self, CratestackError> {
        if !is_mac(alg) {
            return Err(CratestackError::Validation(
                "an HMAC key needs an HMAC algorithm".to_owned(),
            ));
        }
        Self::hmac_from_secret(alg, HmacSecret::new(secret)?)
    }

    /// As [`hmac`](Self::hmac), from an already validated [`HmacSecret`]
    /// (the counterpart of `HmacSigner::from_secret`), so a secret loaded
    /// once can back a signer and a verification key without its bytes
    /// passing through a plain `Vec` again. Fails with
    /// `CratestackError::Validation` for a non-HMAC `alg`.
    pub fn hmac_from_secret(alg: CoseAlg, secret: HmacSecret) -> Result<Self, CratestackError> {
        if !is_mac(alg) {
            return Err(CratestackError::Validation(
                "an HMAC key needs an HMAC algorithm".to_owned(),
            ));
        }
        Ok(Self::from_hmac_secret(alg, secret))
    }

    pub(crate) fn from_ed25519(key: ed25519_dalek::VerifyingKey) -> Self {
        let thumbprint = thumbprint::okp_ed25519_thumbprint(key.as_bytes());
        Self {
            repr: Repr::Ed25519(key),
            thumbprint,
        }
    }

    pub(crate) fn from_p256(key: p256::ecdsa::VerifyingKey) -> Self {
        let point = key.to_sec1_point(false);
        let bytes = point.as_bytes();
        let mut x = [0; 32];
        let mut y = [0; 32];
        // Uncompressed SEC1 is `0x04 ‖ x ‖ y`, 65 bytes, for P-256.
        x.copy_from_slice(&bytes[1..33]);
        y.copy_from_slice(&bytes[33..65]);
        Self {
            repr: Repr::P256(key),
            thumbprint: thumbprint::ec2_p256_thumbprint(&x, &y),
        }
    }

    /// `alg` must be a MAC algorithm; callers check.
    pub(crate) fn from_hmac_secret(alg: CoseAlg, secret: HmacSecret) -> Self {
        let thumbprint = thumbprint::symmetric_thumbprint(secret.expose());
        Self {
            repr: Repr::Hmac { alg, secret },
            thumbprint,
        }
    }

    /// The one algorithm this key verifies.
    pub fn alg(&self) -> CoseAlg {
        match &self.repr {
            Repr::Ed25519(_) => CoseAlg::Ed25519,
            Repr::P256(_) => CoseAlg::Esp256,
            Repr::Hmac { alg, .. } => *alg,
        }
    }

    /// Whether this key can verify messages of `alg`: exactly
    /// [`alg`](Self::alg).
    pub fn supports(&self, alg: CoseAlg) -> bool {
        self.alg() == alg
    }

    /// The key's RFC 9679 thumbprint.
    pub fn thumbprint(&self) -> [u8; 32] {
        self.thumbprint
    }

    /// The key's `kid` (§3): the first 8 bytes of its thumbprint.
    pub fn kid(&self) -> [u8; KID_LEN] {
        thumbprint::kid_from_thumbprint(&self.thumbprint)
    }

    /// The 32-byte Ed25519 public key, or `None` for another key type.
    pub fn ed25519_bytes(&self) -> Option<[u8; 32]> {
        match &self.repr {
            Repr::Ed25519(key) => Some(key.to_bytes()),
            _ => None,
        }
    }

    /// The uncompressed SEC1 P-256 point (`0x04 ‖ x ‖ y`, 65 bytes), or
    /// `None` for another key type.
    pub fn p256_sec1_uncompressed(&self) -> Option<[u8; 65]> {
        match &self.repr {
            Repr::P256(key) => key.to_sec1_point(false).as_bytes().try_into().ok(),
            _ => None,
        }
    }
}

impl fmt::Debug for CoseVerifyKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CoseVerifyKey")
            .field("alg", &self.alg())
            .field("kid", &self.kid())
            .finish()
    }
}
