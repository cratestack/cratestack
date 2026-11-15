//! The keystore callback signer: an ESP256 key that never leaves the
//! platform keystore (cratestack#1151).
//!
//! The Dart side hands over the key's *public* half and a callback that
//! signs; this crate's `ExternalSigner` asks it to sign the complete
//! `Sig_structure`, accepts DER or raw `r ‖ s`, normalises to low-`s`, and
//! checks the result verifies before anything is sent. All of that is
//! `cratestack-cose`'s, so Dart does none of it.

use std::future::Future;
use std::sync::Arc;

use cratestack_core::CratestackError;
use cratestack_cose::{CoseVerifyKey, ExternalSigner};
#[cfg(feature = "frb-glue")]
use flutter_rust_bridge::DartFnFuture;

use super::envelope::FlutterClientEnvelope;
use super::error::FlutterCoseError;
use super::sign_outcome::FlutterSignOutcome;
use super::types::{FlutterSealOptions, FlutterServerKey};
use crate::cose_outcome as outcome;

impl FlutterClientEnvelope {
    /// A COSE_Sign1 (ESP256) envelope whose key lives in a platform
    /// keystore. `public_key` is a SEC1 P-256 point (uncompressed, 65 bytes,
    /// or compressed, 33 bytes) or a P-256 `SubjectPublicKeyInfo` (what
    /// Android's `getEncoded()` returns); the `kid` is derived from it. The
    /// server keys in `server_keys` are SEC1 only: no SPKI. `sign` is asked for an ECDSA/SHA-256
    /// signature over the bytes it is given, as `SHA256withECDSA` does, and
    /// returns a [`FlutterSignOutcome`]: it must not throw.
    ///
    /// Not bridged as such (it is generic); see `esp256_external`.
    #[cfg_attr(feature = "frb-glue", flutter_rust_bridge::frb(ignore))]
    pub fn esp256_with<F, Fut>(
        public_key: Vec<u8>,
        sign: F,
        server_keys: Vec<FlutterServerKey>,
        audience: String,
        options: Option<FlutterSealOptions>,
    ) -> Result<Self, FlutterCoseError>
    where
        F: Fn(Vec<u8>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = FlutterSignOutcome> + Send + 'static,
    {
        let point = CoseVerifyKey::p256_public(&public_key)?
            .p256_sec1_uncompressed()
            .ok_or_else(|| CratestackError::Validation("invalid P-256 public key".to_owned()))?;
        let signer = ExternalSigner::esp256(&point, move |tbs| {
            // Taken now, while the seal is being polled; the answer may
            // arrive in a later poll.
            let slot = outcome::current();
            let answer = sign(tbs);
            async move {
                let FlutterSignOutcome {
                    kind,
                    signature,
                    message,
                } = answer.await;
                match FlutterCoseError::signer(kind, message) {
                    None => Ok(signature),
                    Some(error) => {
                        if let Some(slot) = slot {
                            slot.record(error);
                        }
                        Err(CratestackError::Internal(
                            "the keystore signer did not sign".to_owned(),
                        ))
                    }
                }
            }
        })?;
        Self::from_signer(Arc::new(signer), server_keys, audience, options)
    }

    /// As [`esp256_with`](Self::esp256_with), with the callback as a Dart
    /// function: the bridged constructor `ExternalSigner` in
    /// `package:cratestack_cbor/cose.dart` calls.
    ///
    /// Async because a `DartFn` callback is called from the seal's future,
    /// and the Dart side already awaits envelope creation.
    #[cfg(feature = "frb-glue")]
    pub async fn esp256_external(
        public_key: Vec<u8>,
        sign: impl Fn(Vec<u8>) -> DartFnFuture<FlutterSignOutcome> + Send + Sync + 'static,
        server_keys: Vec<FlutterServerKey>,
        audience: String,
        options: Option<FlutterSealOptions>,
    ) -> Result<Self, FlutterCoseError> {
        Self::esp256_with(public_key, sign, server_keys, audience, options)
    }
}
