//! A signer whose key lives outside this process (cratestack#1007).
//!
//! [`ExternalSigner::esp256`] for a P-256 key in a platform keystore, and
//! [`ExternalSigner::ed25519`] for an Ed25519 key behind a callback (a Node
//! `KeyObject` that cannot be exported, a KMS, a hardware token).

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use cratestack_core::CratestackError;

use super::external_signature::{ed25519_signature, raw_signature};
use super::traits::CoseSigner;
use super::verify::esp256_low_s;
use super::verify_key::CoseVerifyKey;
use crate::alg::CoseAlg;
use crate::maybe_send::{BoxFuture, MaybeSend, MaybeSendSync};
use crate::thumbprint::KID_LEN;

#[cfg(not(target_arch = "wasm32"))]
type SignFn =
    dyn Fn(Vec<u8>) -> BoxFuture<'static, Result<Vec<u8>, CratestackError>> + Send + Sync + 'static;
#[cfg(target_arch = "wasm32")]
type SignFn = dyn Fn(Vec<u8>) -> BoxFuture<'static, Result<Vec<u8>, CratestackError>> + 'static;

/// A signer that hands the bytes to a callback: a key in the Android Keystore
/// or the iOS Secure Enclave, a KMS, a hardware token.
///
/// The key is never exported; the callback is asked to sign the complete
/// `Sig_structure` and nothing else. On `wasm32` neither the callback nor its
/// future needs to be `Send` (see [`CoseSigner`]).
///
/// An Ed25519 key behind a callback, with the platform's own primitive
/// standing in for the key store (here, `Ed25519Signer`):
///
/// ```
/// use cratestack_cose::{CoseAlg, CoseSigner, Ed25519Signer, ExternalSigner};
///
/// let key = Ed25519Signer::from_seed(&[7; 32]);
/// let public = key.verify_key().ed25519_bytes().unwrap();
/// let signer = ExternalSigner::ed25519(&public, move |tbs| {
///     let key = key.clone();
///     async move { key.sign(&tbs).await }
/// })
/// .unwrap();
/// assert_eq!(signer.alg(), CoseAlg::Ed25519);
/// assert_eq!(signer.kid().len(), 8);
/// ```
///
/// A P-256 key:
///
/// ```
/// use cratestack_cose::{CoseAlg, CoseSigner, ExternalSigner, P256Signer};
///
/// // A stand-in for a platform keystore.
/// let key = P256Signer::from_scalar(&[7; 32]).unwrap();
/// let public = key.verify_key().p256_sec1_uncompressed().unwrap();
/// let signer = ExternalSigner::esp256(&public, move |tbs| {
///     let key = key.clone();
///     async move { key.sign(&tbs).await }
/// })
/// .unwrap();
/// assert_eq!(signer.alg(), CoseAlg::Esp256);
/// assert_eq!(signer.kid().len(), 8);
/// ```
#[derive(Clone)]
pub struct ExternalSigner {
    alg: CoseAlg,
    kid: [u8; KID_LEN],
    /// The public half the callback is held to: a signature it returns that
    /// does not verify under it is refused here, not left to fail at the
    /// peer as the coarse, unexplained `401`.
    key: CoseVerifyKey,
    sign: Arc<SignFn>,
}

impl ExternalSigner {
    /// An ESP256 (`-9`) signer for the P-256 key whose public half is
    /// `public_key_sec1` (compressed or uncompressed SEC1). The `kid` is the
    /// key's RFC 9679 thumbprint prefix, computed here so the caller cannot
    /// get it wrong.
    ///
    /// `sign` receives the full to-be-signed bytes and must sign them with
    /// ECDSA over SHA-256 (`SHA256withECDSA` on Android, `ecdsaSignatureMessageX962SHA256`
    /// on iOS: both hash for you, so pass the bytes as given, never a
    /// digest). It may return the signature DER encoded, which is what both
    /// platform keystores produce, or as the raw 64-byte `r ‖ s`. A DER
    /// signature is converted; the envelope then normalises to low-`s`. A
    /// signature that does not verify under `public_key_sec1` is refused
    /// here, on every call, instead of failing at the peer as the coarse `401`.
    ///
    /// Fails with `CratestackError::Validation` if `public_key_sec1` is not a
    /// point on the curve.
    pub fn esp256<F, Fut>(public_key_sec1: &[u8], sign: F) -> Result<Self, CratestackError>
    where
        F: Fn(Vec<u8>) -> Fut + MaybeSendSync + 'static,
        Fut: Future<Output = Result<Vec<u8>, CratestackError>> + MaybeSend + 'static,
    {
        let key = CoseVerifyKey::p256_sec1(public_key_sec1)?;
        let kid = key.kid();
        Ok(Self {
            alg: CoseAlg::Esp256,
            kid,
            key,
            sign: Arc::new(move |tbs| Box::pin(sign(tbs))),
        })
    }

    /// An Ed25519 (`-19`) signer for the key whose public half is
    /// `public_key`. The `kid` is the key's RFC 9679 thumbprint prefix,
    /// computed here so the caller cannot get it wrong.
    ///
    /// `sign` receives the full to-be-signed bytes and must return the
    /// 64-byte Ed25519 signature over exactly those bytes (pure Ed25519, as
    /// RFC 8032 defines it, never a pre-hash of them), which is what a Node
    /// `crypto.sign(null, tbs, key)` or a KMS `ED25519` key produces. Any
    /// other length is refused as the signer's failure, and so is a
    /// signature that does not verify under `public_key`, which is checked
    /// here on every call instead of failing at the peer as the coarse `401`.
    ///
    /// Fails with `CratestackError::Validation` if `public_key` is not a
    /// point encoding.
    pub fn ed25519<F, Fut>(public_key: &[u8; 32], sign: F) -> Result<Self, CratestackError>
    where
        F: Fn(Vec<u8>) -> Fut + MaybeSendSync + 'static,
        Fut: Future<Output = Result<Vec<u8>, CratestackError>> + MaybeSend + 'static,
    {
        let key = CoseVerifyKey::ed25519(public_key)?;
        let kid = key.kid();
        Ok(Self {
            alg: CoseAlg::Ed25519,
            kid,
            key,
            sign: Arc::new(move |tbs| Box::pin(sign(tbs))),
        })
    }
}

impl fmt::Debug for ExternalSigner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExternalSigner")
            .field("alg", &self.alg)
            .field("kid", &self.kid)
            .finish_non_exhaustive()
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl CoseSigner for ExternalSigner {
    fn alg(&self) -> CoseAlg {
        self.alg
    }

    fn kid(&self) -> &[u8] {
        &self.kid
    }

    async fn sign(&self, to_be_signed: &[u8]) -> Result<Vec<u8>, CratestackError> {
        let signature = (self.sign)(to_be_signed.to_vec()).await?;
        let signature = match self.alg {
            CoseAlg::Ed25519 => ed25519_signature(signature)?,
            _ => raw_signature(&signature)?,
        };
        // ESP256 is checked as the envelope will send it, with a low `s`.
        let checked = match self.alg {
            CoseAlg::Ed25519 => Some(signature.clone()),
            _ => esp256_low_s(&signature),
        };
        match checked {
            Some(checked) if self.key.verify_message(self.alg, to_be_signed, &checked) => {
                Ok(signature)
            }
            _ => Err(CratestackError::Internal(
                "the external signer's signature does not verify under the public key it was \
                 built with"
                    .to_owned(),
            )),
        }
    }
}
