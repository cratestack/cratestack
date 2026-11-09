//! Constructors of `ClientEnvelope` beyond the HMAC and Ed25519-seed ones
//! (issue #1010). The main one is a `ClientEnvelope` whose private key never
//! reaches this module: a non-extractable WebCrypto key, a platform keystore, a KMS. The
//! caller supplies `sign(toBeSigned) => Promise<Uint8Array>`; the module
//! hands it the complete to-be-signed bytes, checks what comes back under the
//! public key, and does everything else itself (the CBOR, the AAD, ESP256's
//! low-`s` normalisation, DER to raw `r ‖ s`). This is
//! `cratestack_cose::ExternalSigner`, with a JS function as the callback.

use cratestack_core::CratestackError;
use cratestack_cose::{CoseAlg, ExternalSigner, P256Signer};
use js_sys::{Function, Promise, Uint8Array};
use std::sync::Arc;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use zeroize::Zeroizing;

use super::convert;
use super::envelope::ClientEnvelope;
use super::error::{from_error, misuse};

#[wasm_bindgen]
impl ClientEnvelope {
    /// A COSE_Sign1 envelope signing with the P-256 key of a 32-byte
    /// `scalar`, held in memory (ESP256, low-`s`). For a key that must not
    /// be in memory, use [`external`](Self::external).
    #[wasm_bindgen(js_name = p256Scalar)]
    pub fn p256_scalar(
        scalar: &[u8],
        server_keys: &JsValue,
        audience: &str,
        options: &JsValue,
    ) -> Result<ClientEnvelope, JsValue> {
        // The copy is wiped on drop.
        let scalar: Zeroizing<[u8; 32]> = Zeroizing::new(
            scalar
                .try_into()
                .map_err(|_| misuse("a P-256 scalar is 32 bytes"))?,
        );
        let signer = P256Signer::from_scalar(&scalar).map_err(from_error)?;
        Self::build(Arc::new(signer), server_keys, audience, options)
    }

    /// A COSE_Sign1 envelope whose signature comes from `sign`.
    ///
    /// `alg` is `"esp256"` (`publicKey` is the SEC1 point, compressed or
    /// uncompressed; `sign` returns ECDSA over SHA-256 of the bytes it is
    /// given, DER or raw `r ‖ s`, which is what WebCrypto's
    /// `subtle.sign({ name: "ECDSA", hash: "SHA-256" }, ...)` returns) or
    /// `"ed25519"` (`publicKey` is 32 bytes; `sign` returns the 64-byte
    /// signature over the bytes as given). A signature that does not verify
    /// under `publicKey` is refused here and never sent.
    #[wasm_bindgen(js_name = external)]
    pub fn external(
        alg: &str,
        public_key: &[u8],
        sign: Function,
        server_keys: &JsValue,
        audience: &str,
        options: &JsValue,
    ) -> Result<ClientEnvelope, JsValue> {
        let callback = move |to_be_signed: Vec<u8>| {
            let sign = sign.clone();
            async move { call_sign(&sign, &to_be_signed).await }
        };
        let signer = match convert::alg(alg)? {
            CoseAlg::Esp256 => ExternalSigner::esp256(public_key, callback),
            CoseAlg::Ed25519 => {
                let public: [u8; 32] = public_key
                    .try_into()
                    .map_err(|_| misuse("an Ed25519 public key is 32 bytes"))?;
                ExternalSigner::ed25519(&public, callback)
            }
            _ => return Err(misuse("an external signer is `esp256` or `ed25519`")),
        }
        .map_err(from_error)?;
        // A JS callback is not `Send`: on `wasm32` neither is the signer, by
        // design (see `CoseSigner`), and there is one thread.
        #[allow(clippy::arc_with_non_send_sync)]
        let signer = Arc::new(signer);
        Self::build(signer, server_keys, audience, options)
    }
}

/// Calls the JS signer. A throw, a rejection or a result that is not a
/// `Uint8Array` is the signer's failure (`misuse`), without the page's
/// own error text, which could carry anything.
async fn call_sign(sign: &Function, to_be_signed: &[u8]) -> Result<Vec<u8>, CratestackError> {
    let failed = |what: &str| CratestackError::Internal(format!("the signing callback {what}"));
    let returned = sign
        .call1(&JsValue::NULL, &Uint8Array::from(to_be_signed))
        .map_err(|_| failed("threw"))?;
    let settled = JsFuture::from(Promise::resolve(&returned))
        .await
        .map_err(|_| failed("rejected"))?;
    settled
        .dyn_into::<Uint8Array>()
        .map(|signature| signature.to_vec())
        .map_err(|_| failed("did not return a Uint8Array"))
}
