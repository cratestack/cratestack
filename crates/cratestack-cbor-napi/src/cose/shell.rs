//! The napi class around [`Envelope`]. No logic of its own: it converts the
//! JS values to `spec.rs` shapes and the results back.

use napi::bindgen_prelude::Uint8Array;
use napi_derive::napi;

use super::envelope::{Envelope, OpenedResponse};
use super::fail::Fail;
use super::inputs::{
    BindingInput, HeaderPair, OpenedOutput, SealOptionsInput, ServerKeyInput, binding, keys,
    options,
};
use super::pending::PendingSeal;

/// `napi::Error` carrying the `{ code, message }` of a [`Fail`]: the JS
/// error's `code` is `"rejected"` or `"misuse"`.
type Failure = napi::Error<String>;

fn failure(fail: Fail) -> Failure {
    napi::Error::new(fail.code.to_owned(), fail.message)
}

/// Seals requests and opens responses for one service (ADR 0006).
#[napi]
pub struct ClientEnvelope {
    inner: Envelope,
}

#[napi]
impl ClientEnvelope {
    /// A COSE_Mac0 envelope over a shared secret of at least 32 bytes.
    #[napi(factory, catch_unwind)]
    pub fn hmac(
        alg: String,
        secret: Uint8Array,
        server_keys: Vec<ServerKeyInput>,
        audience: String,
        options_: Option<SealOptionsInput>,
    ) -> napi::Result<ClientEnvelope, String> {
        Envelope::hmac(
            &alg,
            &secret,
            &keys(server_keys),
            &audience,
            options(options_),
        )
        .map(|inner| ClientEnvelope { inner })
        .map_err(failure)
    }

    /// A COSE_Sign1 envelope signing with the Ed25519 key of a 32-byte seed.
    #[napi(factory, catch_unwind)]
    pub fn ed25519_seed(
        seed: Uint8Array,
        server_keys: Vec<ServerKeyInput>,
        audience: String,
        options_: Option<SealOptionsInput>,
    ) -> napi::Result<ClientEnvelope, String> {
        Envelope::ed25519_seed(&seed, &keys(server_keys), &audience, options(options_))
            .map(|inner| ClientEnvelope { inner })
            .map_err(failure)
    }

    /// A COSE_Sign1 envelope signing with the P-256 key of a 32-byte scalar.
    #[napi(factory, catch_unwind)]
    pub fn p256_scalar(
        scalar: Uint8Array,
        server_keys: Vec<ServerKeyInput>,
        audience: String,
        options_: Option<SealOptionsInput>,
    ) -> napi::Result<ClientEnvelope, String> {
        Envelope::p256_scalar(&scalar, &keys(server_keys), &audience, options(options_))
            .map(|inner| ClientEnvelope { inner })
            .map_err(failure)
    }

    /// A COSE_Sign1 envelope whose signature the caller makes (`"esp256"`
    /// or `"ed25519"`): seal it with `beginSealRequest`, then `finish`.
    #[napi(factory, catch_unwind)]
    pub fn external(
        alg: String,
        public_key: Uint8Array,
        server_keys: Vec<ServerKeyInput>,
        audience: String,
        options_: Option<SealOptionsInput>,
    ) -> napi::Result<ClientEnvelope, String> {
        Envelope::external(
            &alg,
            &public_key,
            &keys(server_keys),
            &audience,
            options(options_),
        )
        .map(|inner| ClientEnvelope { inner })
        .map_err(failure)
    }

    /// `"sign1"` or `"mac0"`.
    #[napi(getter)]
    pub fn mode(&self) -> String {
        self.inner.mode().to_owned()
    }

    /// The `Content-Type` (and `Accept`) of sealed bodies.
    #[napi(getter)]
    pub fn media_type(&self) -> String {
        self.inner.media_type().to_owned()
    }

    /// The signer's 8-byte `kid`.
    #[napi(getter)]
    pub fn kid(&self) -> Uint8Array {
        Uint8Array::from(self.inner.kid().to_vec())
    }

    /// Seal `payload` (already encoded) as the request `binding` describes.
    #[napi(catch_unwind)]
    pub fn seal_request(
        &self,
        payload: Uint8Array,
        binding_: BindingInput,
    ) -> napi::Result<Uint8Array, String> {
        self.inner
            .seal_request(&payload, &binding(binding_))
            .map(Uint8Array::from)
            .map_err(failure)
    }

    /// The first step of sealing with an external signer.
    #[napi(catch_unwind)]
    pub fn begin_seal_request(
        &self,
        payload: Uint8Array,
        binding_: BindingInput,
    ) -> napi::Result<PendingSeal, String> {
        self.inner
            .begin_seal(&payload, &binding(binding_))
            .map(|inner| PendingSeal { inner })
            .map_err(failure)
    }

    /// The payload-type selector headers the call needs sent.
    #[napi(catch_unwind)]
    pub fn selector_headers(
        &self,
        binding_: BindingInput,
    ) -> napi::Result<Vec<HeaderPair>, String> {
        self.inner
            .selector_headers(&binding(binding_))
            .map(|pairs| {
                pairs
                    .into_iter()
                    .map(|(name, value)| HeaderPair { name, value })
                    .collect()
            })
            .map_err(failure)
    }

    /// Open the response `body` to `sealedRequest` (the exact bytes that
    /// were sent), which came back with HTTP `status`.
    #[napi(catch_unwind)]
    pub fn open_response(
        &self,
        body: Uint8Array,
        binding_: BindingInput,
        sealed_request: Uint8Array,
        status: u16,
        response_payload_type: Option<String>,
    ) -> napi::Result<OpenedOutput, String> {
        let OpenedResponse {
            payload,
            payload_type,
            kid,
            alg,
            thumbprint,
        } = self
            .inner
            .open_response(
                &body,
                &binding(binding_),
                &sealed_request,
                status,
                response_payload_type.as_deref(),
            )
            .map_err(failure)?;
        Ok(OpenedOutput {
            payload: Uint8Array::from(payload),
            payload_type,
            kid: Uint8Array::from(kid),
            alg: alg.to_owned(),
            thumbprint: Uint8Array::from(thumbprint),
        })
    }
}
