//! A signer whose key lives outside this process (cratestack#1007).

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use cratestack_core::CratestackError;
use p256::ecdsa::Signature;

use super::traits::CoseSigner;
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
    /// signature is converted; the envelope then normalises to low-`s`.
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
        raw_signature(&signature)
    }
}

/// The 64-byte `r ‖ s` for a signature that is either that or DER.
///
/// DER is tried first when the bytes start a DER `SEQUENCE` (`0x30`) and
/// parse as one; a raw signature that happens to start with `0x30` does not
/// parse as a DER signature of the same length, so the two cannot be mixed
/// up.
fn raw_signature(signature: &[u8]) -> Result<Vec<u8>, CratestackError> {
    let parsed = match signature {
        [0x30, ..] => Signature::from_der(signature).ok(),
        _ => None,
    }
    .or_else(|| Signature::from_slice(signature).ok());
    parsed
        .map(|signature| signature.to_bytes().to_vec())
        .ok_or_else(|| {
            CratestackError::Internal(
                "the external signer returned neither DER nor a raw 64-byte P-256 signature"
                    .to_owned(),
            )
        })
}
