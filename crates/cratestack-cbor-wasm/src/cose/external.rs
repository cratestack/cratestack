//! Constructors of `ClientEnvelope` beyond the HMAC and Ed25519-seed ones
//! (issue #1010). The main one is a `ClientEnvelope` whose private key never
//! reaches this module: a non-extractable WebCrypto key, a platform keystore, a KMS. The
//! caller supplies `sign(toBeSigned) => Promise<Uint8Array>`; the module
//! hands it the complete to-be-signed bytes, checks what comes back under the
//! public key, and does everything else itself (the CBOR, the AAD, ESP256's
//! low-`s` normalisation, DER to raw `r ‖ s`). This is
//! `cratestack_cose::ExternalSigner`, with a JS function as the callback.

use cratestack_core::CratestackError;
use cratestack_cose::{CoseAlg, CoseVerifyKey, ExternalSigner, P256Signer};
use js_sys::{Function, Promise, Uint8Array};
use std::sync::Arc;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use zeroize::Zeroizing;

use super::convert;
use super::envelope::ClientEnvelope;
use super::error::{from_error, misuse};
use super::outcome;

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
    /// `alg` is `"esp256"` (`publicKey` is a SEC1 point, compressed or
    /// uncompressed, or a P-256 `SubjectPublicKeyInfo` as Android's
    /// `getEncoded()` gives; `sign` returns ECDSA over SHA-256 of the bytes it is
    /// given, DER or raw `r ‖ s`, which is what WebCrypto's
    /// `subtle.sign({ name: "ECDSA", hash: "SHA-256" }, ...)` returns) or
    /// `"ed25519"` (`publicKey` is 32 bytes; `sign` returns the 64-byte
    /// signature over the bytes as given). A signature that does not verify
    /// under `publicKey` is refused here and never sent.
    ///
    /// `sign` may also resolve to `{ outcome: "cancelled" | "timedOut" |
    /// "failed", message?: string }` instead of a signature (the Dart
    /// package's keystore signer does). The seal then rejects with code
    /// `signer_cancelled`, `signer_timed_out` or `signer_failed` (with the
    /// message); a callback that only ever returns bytes never sees these.
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
            // Taken now, while the seal is being polled.
            let slot = outcome::current();
            async move { call_sign(&sign, &to_be_signed, slot).await }
        };
        let signer = match convert::alg(alg)? {
            CoseAlg::Esp256 => CoseVerifyKey::p256_public(public_key)
                .and_then(|key| {
                    key.p256_sec1_uncompressed().ok_or_else(|| {
                        CratestackError::Validation("invalid P-256 public key".to_owned())
                    })
                })
                .and_then(|point| ExternalSigner::esp256(&point, callback)),
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

/// Calls the JS signer. A throw, a rejection or a result that is neither a
/// `Uint8Array` nor a tagged outcome is the signer's failure (`misuse`),
/// without the page's own error text, which could carry anything. A tagged
/// outcome is recorded in `slot` and fails the seal as that kind.
async fn call_sign(
    sign: &Function,
    to_be_signed: &[u8],
    slot: Option<outcome::Slot>,
) -> Result<Vec<u8>, CratestackError> {
    let failed = |what: &str| CratestackError::Internal(format!("the signing callback {what}"));
    let returned = sign
        .call1(&JsValue::NULL, &Uint8Array::from(to_be_signed))
        .map_err(|_| failed("threw"))?;
    let settled = JsFuture::from(Promise::resolve(&returned))
        .await
        .map_err(|_| failed("rejected"))?;
    match settled.dyn_into::<Uint8Array>() {
        Ok(signature) => Ok(signature.to_vec()),
        Err(settled) => match (outcome::parse(&settled), slot) {
            (Some((code, message)), Some(slot)) => {
                slot.record(code, message);
                Err(failed("did not sign"))
            }
            _ => Err(failed("did not return a Uint8Array")),
        },
    }
}
