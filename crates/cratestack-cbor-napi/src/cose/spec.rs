//! Plain-Rust shapes of what JavaScript passes in, and their conversion to
//! `cratestack-cose` types. No napi types here, so all of it runs under a
//! host `cargo test` (see the module docs of `lib.rs`).

use cratestack_cose::{CallBinding, CoseAlg, CoseVerifyKey};

use super::fail::Fail;

/// The algorithm names of the JS API, the same as the web build's.
pub(crate) fn alg(name: &str) -> Result<CoseAlg, Fail> {
    match name {
        "ed25519" => Ok(CoseAlg::Ed25519),
        "esp256" => Ok(CoseAlg::Esp256),
        "hmac256-64" => Ok(CoseAlg::Hmac256_64),
        "hmac256-256" => Ok(CoseAlg::Hmac256_256),
        other => Err(Fail::misuse(format!("unknown algorithm `{other}`"))),
    }
}

/// The inverse of [`alg`]. `CoseAlg` is non-exhaustive, so a value this API
/// has no name for is misuse, never a guess.
pub(crate) fn alg_name(alg: CoseAlg) -> Result<&'static str, Fail> {
    match alg {
        CoseAlg::Ed25519 => Ok("ed25519"),
        CoseAlg::Esp256 => Ok("esp256"),
        CoseAlg::Hmac256_64 => Ok("hmac256-64"),
        CoseAlg::Hmac256_256 => Ok("hmac256-256"),
        _ => Err(Fail::misuse("an algorithm this API has no name for")),
    }
}

/// One trusted server key: `{ alg, bytes }` (an Ed25519 public key, a SEC1
/// point or an HMAC secret).
pub(crate) struct ServerKey {
    pub(crate) alg: String,
    pub(crate) bytes: Vec<u8>,
}

pub(crate) fn server_keys(keys: &[ServerKey]) -> Result<Vec<CoseVerifyKey>, Fail> {
    keys.iter()
        .map(|entry| {
            let alg = alg(&entry.alg)?;
            match alg {
                CoseAlg::Ed25519 => {
                    let public: [u8; 32] = entry
                        .bytes
                        .as_slice()
                        .try_into()
                        .map_err(|_| Fail::misuse("an Ed25519 server key is 32 bytes"))?;
                    CoseVerifyKey::ed25519(&public)
                }
                CoseAlg::Esp256 => CoseVerifyKey::p256_sec1(&entry.bytes),
                _ => CoseVerifyKey::hmac(alg, entry.bytes.clone()),
            }
            .map_err(Fail::from)
        })
        .collect()
}

/// `{ fixedIat?, fixedCti? }`, for tests that must reproduce the shared
/// vectors.
#[derive(Default)]
pub(crate) struct SealOptions {
    pub(crate) fixed_iat: Option<i64>,
    pub(crate) fixed_cti: Option<Vec<u8>>,
}

/// `{ method, route, pathParams, query?, contractSha, idempotencyKey?,
/// ifMatch?, payloadType?, payloadAccept? }`.
pub(crate) struct BindingSpec {
    pub(crate) method: String,
    pub(crate) route: String,
    pub(crate) path_params: Vec<String>,
    pub(crate) query: Option<String>,
    pub(crate) contract_sha: Vec<u8>,
    pub(crate) idempotency_key: Option<String>,
    pub(crate) if_match: Option<String>,
    pub(crate) payload_type: Option<String>,
    pub(crate) payload_accept: Option<Vec<String>>,
}

impl BindingSpec {
    /// The binding addressed to `audience`. `payloadType` and
    /// `payloadAccept` default to CBOR; both are validated here, before
    /// anything is sealed.
    pub(crate) fn to_call(&self, audience: &str) -> Result<CallBinding, Fail> {
        let contract_sha: [u8; 32] = self
            .contract_sha
            .as_slice()
            .try_into()
            .map_err(|_| Fail::misuse("`contractSha` is 32 bytes"))?;
        let call = CallBinding {
            audience: audience.to_owned(),
            method: self.method.clone(),
            route: self.route.clone(),
            path_params: self.path_params.clone(),
            query: self.query.clone(),
            contract_sha,
            idempotency_key: self.idempotency_key.clone(),
            if_match: self.if_match.clone(),
            payload_type: self.payload_type.clone(),
            payload_accept: self.payload_accept.clone(),
        };
        call.validate()?;
        Ok(call)
    }
}
