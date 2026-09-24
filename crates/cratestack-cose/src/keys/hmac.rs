//! HMAC secrets and the in-process COSE_Mac0 signer.

use std::fmt;

use cratestack_core::CratestackError;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

use super::traits::CoseSigner;
use crate::alg::CoseAlg;
use crate::thumbprint::{self, KID_LEN};

/// The shortest HMAC secret accepted, in bytes.
///
/// 32 bytes is SHA-256's output size, the key length RFC 2104 recommends
/// as a floor. It is also what makes the `kid` safe to publish: a Mac0
/// `kid` is a prefix of the secret's RFC 9679 thumbprint, and RFC 9679 §7
/// forbids thumbprints of low-entropy secrets because a short one could be
/// found by hashing candidates (P0 scoping decision).
pub const MIN_HMAC_SECRET_LEN: usize = 32;

/// An HMAC secret of at least [`MIN_HMAC_SECRET_LEN`] bytes. `Debug` does
/// not print it, and `==` compares in constant time.
#[derive(Clone)]
pub struct HmacSecret(Vec<u8>);

impl PartialEq for HmacSecret {
    fn eq(&self, other: &Self) -> bool {
        bool::from(self.0.as_slice().ct_eq(other.0.as_slice()))
    }
}

impl Eq for HmacSecret {}

impl HmacSecret {
    pub fn new(secret: impl Into<Vec<u8>>) -> Result<Self, CratestackError> {
        let secret = secret.into();
        if secret.len() < MIN_HMAC_SECRET_LEN {
            return Err(CratestackError::Validation(format!(
                "HMAC secret must be at least {MIN_HMAC_SECRET_LEN} bytes"
            )));
        }
        Ok(Self(secret))
    }

    pub(crate) fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for HmacSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("HmacSecret(<redacted>)")
    }
}

/// HMAC-SHA-256 over `data`, truncated to `alg`'s tag length (the first 8
/// bytes for HMAC 256/64, RFC 9053 §3.1). Only called with a MAC `alg`.
pub(crate) fn hmac_tag(secret: &HmacSecret, alg: CoseAlg, data: &[u8]) -> Vec<u8> {
    // `new_from_slice` cannot fail for HMAC: any key length is valid.
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.expose()) else {
        return Vec::new();
    };
    mac.update(data);
    let full = mac.finalize().into_bytes();
    full[..alg.signature_len().min(full.len())].to_vec()
}

/// The in-process COSE_Mac0 signer: service to service inside one trust
/// domain (§3). Its `kid` is the thumbprint prefix of the secret.
#[derive(Clone)]
pub struct HmacSigner {
    secret: HmacSecret,
    alg: CoseAlg,
    kid: [u8; KID_LEN],
}

impl HmacSigner {
    /// `alg` must be [`CoseAlg::Hmac256_64`] or [`CoseAlg::Hmac256_256`],
    /// and the secret at least 32 bytes.
    pub fn new(alg: CoseAlg, secret: impl Into<Vec<u8>>) -> Result<Self, CratestackError> {
        if !matches!(alg, CoseAlg::Hmac256_64 | CoseAlg::Hmac256_256) {
            return Err(CratestackError::Validation(
                "HmacSigner needs an HMAC algorithm".to_owned(),
            ));
        }
        let secret = HmacSecret::new(secret)?;
        let kid =
            thumbprint::kid_from_thumbprint(&thumbprint::symmetric_thumbprint(secret.expose()));
        Ok(Self { secret, alg, kid })
    }

    /// The matching verification key, for a resolver.
    pub fn verify_key(&self) -> super::CoseVerifyKey {
        super::CoseVerifyKey::Hmac(self.secret.clone())
    }
}

impl fmt::Debug for HmacSigner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HmacSigner")
            .field("alg", &self.alg)
            .field("kid", &self.kid)
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl CoseSigner for HmacSigner {
    fn alg(&self) -> CoseAlg {
        self.alg
    }

    fn kid(&self) -> &[u8] {
        &self.kid
    }

    async fn sign(&self, to_be_signed: &[u8]) -> Result<Vec<u8>, CratestackError> {
        Ok(hmac_tag(&self.secret, self.alg, to_be_signed))
    }
}
