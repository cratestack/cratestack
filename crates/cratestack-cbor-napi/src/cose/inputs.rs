//! The plain objects JavaScript passes in and gets back, and their
//! conversion to the `spec.rs` shapes. No logic of its own.

use napi::bindgen_prelude::Uint8Array;
use napi_derive::napi;

use super::spec::{BindingSpec, SealOptions, ServerKey};

/// A trusted server key: an Ed25519 public key, a SEC1 P-256 point or an
/// HMAC secret.
#[napi(object)]
pub struct ServerKeyInput {
    /// `"ed25519"`, `"esp256"`, `"hmac256-64"` or `"hmac256-256"`.
    pub alg: String,
    pub bytes: Uint8Array,
}

/// For tests that must reproduce the shared vectors.
#[napi(object)]
pub struct SealOptionsInput {
    pub fixed_iat: Option<i64>,
    pub fixed_cti: Option<Uint8Array>,
}

/// What a call is bound to (ADR 0006 §4).
#[napi(object)]
pub struct BindingInput {
    pub method: String,
    /// The route template on REST, the op id on RPC.
    pub route: String,
    pub path_params: Vec<String>,
    pub query: Option<String>,
    pub contract_sha: Uint8Array,
    pub idempotency_key: Option<String>,
    pub if_match: Option<String>,
    /// The type the payload is encoded as (default `application/cbor`).
    pub payload_type: Option<String>,
    /// The types the answer may come back in (default CBOR only).
    pub payload_accept: Option<Vec<String>>,
}

/// A verified response.
#[napi(object)]
pub struct OpenedOutput {
    pub payload: Uint8Array,
    pub payload_type: String,
    pub kid: Uint8Array,
    pub alg: String,
    pub thumbprint: Uint8Array,
}

/// A header the seal depends on.
#[napi(object)]
pub struct HeaderPair {
    pub name: String,
    pub value: String,
}

pub(super) fn keys(input: Vec<ServerKeyInput>) -> Vec<ServerKey> {
    input
        .into_iter()
        .map(|key| ServerKey {
            alg: key.alg,
            bytes: key.bytes.to_vec(),
        })
        .collect()
}

pub(super) fn options(input: Option<SealOptionsInput>) -> SealOptions {
    input.map_or_else(SealOptions::default, |input| SealOptions {
        fixed_iat: input.fixed_iat,
        fixed_cti: input.fixed_cti.map(|cti| cti.to_vec()),
    })
}

pub(super) fn binding(input: BindingInput) -> BindingSpec {
    BindingSpec {
        method: input.method,
        route: input.route,
        path_params: input.path_params,
        query: input.query,
        contract_sha: input.contract_sha.to_vec(),
        idempotency_key: input.idempotency_key,
        if_match: input.if_match,
        payload_type: input.payload_type,
        payload_accept: input.payload_accept,
    }
}
