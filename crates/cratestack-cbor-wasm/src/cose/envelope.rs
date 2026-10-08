//! The `ClientEnvelope` class.

use std::sync::Arc;

use bytes::Bytes;
use cratestack_cose::{
    CoseEnvelope, CoseSigner, Ed25519Signer, HmacSigner, Opened, StaticVerifierResolver,
};
use js_sys::{Object, Promise, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use super::convert;
use super::error::{from_error, misuse};

/// Seals requests and opens responses for one service (ADR 0006).
///
/// The key stays in this module's memory: an HMAC secret or an Ed25519
/// seed, so for tests and service credentials, never a device key (a
/// WebCrypto-backed signer is a later release). All crypto, the canonical
/// query and the AAD are `cratestack-cose`'s.
#[wasm_bindgen]
#[derive(Clone)]
pub struct ClientEnvelope {
    inner: CoseEnvelope,
    audience: String,
    kid: Vec<u8>,
}

#[wasm_bindgen]
impl ClientEnvelope {
    /// A COSE_Mac0 envelope over a shared secret of at least 32 bytes.
    /// `alg` is `"hmac256-64"` or `"hmac256-256"`; `serverKeys` is
    /// `[{ alg, bytes }]`; `options` (tests only) is `{ fixedIat?,
    /// fixedCti? }`.
    #[wasm_bindgen(js_name = hmac)]
    pub fn hmac(
        alg: &str,
        secret: &[u8],
        server_keys: &JsValue,
        audience: &str,
        options: &JsValue,
    ) -> Result<ClientEnvelope, JsValue> {
        let signer = HmacSigner::new(convert::alg(alg)?, secret.to_vec()).map_err(from_error)?;
        Self::build(Arc::new(signer), server_keys, audience, options)
    }

    /// A COSE_Sign1 envelope signing with the Ed25519 key of a 32-byte
    /// `seed`, held in memory.
    #[wasm_bindgen(js_name = ed25519Seed)]
    pub fn ed25519_seed(
        seed: &[u8],
        server_keys: &JsValue,
        audience: &str,
        options: &JsValue,
    ) -> Result<ClientEnvelope, JsValue> {
        let seed: [u8; 32] = seed
            .try_into()
            .map_err(|_| misuse("an Ed25519 seed is 32 bytes"))?;
        Self::build(
            Arc::new(Ed25519Signer::from_seed(&seed)),
            server_keys,
            audience,
            options,
        )
    }

    /// `"sign1"` or `"mac0"`.
    #[wasm_bindgen(getter)]
    pub fn mode(&self) -> String {
        match self.inner.mode() {
            cratestack_cose::CoseMode::Sign1 => "sign1",
            cratestack_cose::CoseMode::Mac0 => "mac0",
        }
        .to_owned()
    }

    /// The `Content-Type` (and `Accept`) of sealed bodies.
    #[wasm_bindgen(getter, js_name = mediaType)]
    pub fn media_type(&self) -> String {
        self.inner.mode().media_type().to_owned()
    }

    /// The signer's 8-byte `kid`.
    #[wasm_bindgen(getter)]
    pub fn kid(&self) -> Vec<u8> {
        self.kid.clone()
    }

    /// Seal `payload` (already CBOR) as the request `binding` describes:
    /// `{ method, route, pathParams, query?, contractSha, idempotencyKey?,
    /// ifMatch? }`. Resolves to the sealed bytes.
    #[wasm_bindgen(js_name = sealRequest)]
    pub fn seal_request(&self, payload: &[u8], binding: &JsValue) -> Promise {
        let this = self.clone();
        let payload = payload.to_vec();
        let binding = convert::binding(binding, &self.audience);
        future_to_promise(async move {
            let call = binding?;
            let sealed = this
                .inner
                .seal_request(&payload, &call.request())
                .await
                .map_err(from_error)?;
            Ok(Uint8Array::from(sealed.as_ref()).into())
        })
    }

    /// Open the response `body` to `sealedRequest` (the exact bytes that
    /// were sent), which came back with HTTP `status`. Resolves to
    /// `{ payload, kid, alg, thumbprint }`; every failed check rejects with
    /// the same `{ code: "rejected", message: "" }`.
    #[wasm_bindgen(js_name = openResponse)]
    pub fn open_response(
        &self,
        body: &[u8],
        binding: &JsValue,
        sealed_request: &[u8],
        status: u16,
    ) -> Promise {
        let this = self.clone();
        let body = Bytes::copy_from_slice(body);
        let sealed_request = sealed_request.to_vec();
        let binding = convert::binding(binding, &self.audience);
        future_to_promise(async move {
            let call = binding?;
            let opened = this
                .inner
                .open_response(body, &call.response(&sealed_request, status))
                .await
                .map_err(from_error)?;
            Ok(opened_object(&opened))
        })
    }
}

impl ClientEnvelope {
    pub(super) fn build(
        signer: Arc<dyn CoseSigner>,
        server_keys: &JsValue,
        audience: &str,
        options: &JsValue,
    ) -> Result<ClientEnvelope, JsValue> {
        if audience.is_empty() {
            return Err(misuse("the audience is empty"));
        }
        let mut resolver = StaticVerifierResolver::new();
        for key in convert::server_keys(server_keys)? {
            resolver = resolver.with_key(key);
        }
        let kid = signer.kid().to_vec();
        let mut builder = CoseEnvelope::client(signer.alg().mode(), signer, Arc::new(resolver));
        if let Some((iat, cti)) = convert::seal_options(options)? {
            if let Some(iat) = iat {
                builder = builder.clock(move || iat);
            }
            if let Some(cti) = cti {
                builder = builder.cti_source(move || Ok(cti.clone()));
            }
        }
        Ok(ClientEnvelope {
            inner: builder.build().map_err(from_error)?,
            audience: audience.to_owned(),
            kid,
        })
    }
}

fn opened_object(opened: &Opened) -> JsValue {
    let object = Object::new();
    let set = |name: &str, value: JsValue| {
        // Setting a property on a fresh plain object cannot fail.
        let _ = Reflect::set(&object, &name.into(), &value);
    };
    set("payload", Uint8Array::from(opened.payload.as_ref()).into());
    set("kid", Uint8Array::from(&opened.kid[..]).into());
    set("alg", convert::alg_name(opened.alg).into());
    set(
        "thumbprint",
        Uint8Array::from(&opened.thumbprint[..]).into(),
    );
    object.into()
}

/// The `Cratestack-Contract` header value for a 32-byte op contract digest
/// (11 characters of unpadded base64url). The header is unbound; send it
/// with every sealed call.
#[wasm_bindgen(js_name = contractHeaderValue)]
pub fn contract_header_value(contract_sha: &[u8]) -> Result<String, JsValue> {
    let digest: [u8; 32] = contract_sha
        .try_into()
        .map_err(|_| misuse("`contractSha` is 32 bytes"))?;
    Ok(cratestack_cose::contract_header_value(&digest))
}
