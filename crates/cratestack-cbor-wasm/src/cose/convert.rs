//! JS values in, `cratestack-cose` types out.
//!
//! Read by hand with `js_sys::Reflect` rather than through serde: a
//! `Uint8Array` must stay a byte string, and a wrong shape must name the
//! field, not fail with a serde path.

use cratestack_cose::{CallBinding, CoseAlg, CoseVerifyKey};
use js_sys::{Array, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};

use super::error::misuse;

type Fail = JsValue;

fn field(object: &JsValue, name: &str) -> Result<JsValue, Fail> {
    if !object.is_object() {
        return Err(misuse(&format!("expected an object with `{name}`")));
    }
    Reflect::get(object, &name.into()).map_err(|_| misuse(&format!("cannot read `{name}`")))
}

pub(super) fn string(object: &JsValue, name: &str) -> Result<String, Fail> {
    field(object, name)?
        .as_string()
        .ok_or_else(|| misuse(&format!("`{name}` must be a string")))
}

pub(super) fn opt_string(object: &JsValue, name: &str) -> Result<Option<String>, Fail> {
    let value = field(object, name)?;
    if value.is_undefined() || value.is_null() {
        return Ok(None);
    }
    value
        .as_string()
        .map(Some)
        .ok_or_else(|| misuse(&format!("`{name}` must be a string")))
}

pub(super) fn bytes_value(value: &JsValue, name: &str) -> Result<Vec<u8>, Fail> {
    value
        .dyn_ref::<Uint8Array>()
        .map(Uint8Array::to_vec)
        .ok_or_else(|| misuse(&format!("`{name}` must be a Uint8Array")))
}

pub(super) fn bytes(object: &JsValue, name: &str) -> Result<Vec<u8>, Fail> {
    bytes_value(&field(object, name)?, name)
}

/// The algorithm names of the web API.
pub(super) fn alg(name: &str) -> Result<CoseAlg, Fail> {
    match name {
        "ed25519" => Ok(CoseAlg::Ed25519),
        "esp256" => Ok(CoseAlg::Esp256),
        "hmac256-64" => Ok(CoseAlg::Hmac256_64),
        "hmac256-256" => Ok(CoseAlg::Hmac256_256),
        other => Err(misuse(&format!("unknown algorithm `{other}`"))),
    }
}

/// The inverse of [`alg`]. `CoseAlg` is non-exhaustive (a reserved hybrid
/// algorithm), so a value this API has no name for is misuse, never a guess.
pub(super) fn alg_name(alg: CoseAlg) -> Result<&'static str, Fail> {
    match alg {
        CoseAlg::Ed25519 => Ok("ed25519"),
        CoseAlg::Esp256 => Ok("esp256"),
        CoseAlg::Hmac256_64 => Ok("hmac256-64"),
        CoseAlg::Hmac256_256 => Ok("hmac256-256"),
        _ => Err(misuse("an algorithm this API has no name for")),
    }
}

/// `[{ alg, bytes }]`: Ed25519 public keys, SEC1 points or HMAC secrets.
pub(super) fn server_keys(keys: &JsValue) -> Result<Vec<CoseVerifyKey>, Fail> {
    if !Array::is_array(keys) {
        return Err(misuse("`serverKeys` must be an array"));
    }
    Array::from(keys)
        .iter()
        .map(|entry| {
            let alg = alg(&string(&entry, "alg")?)?;
            let bytes = bytes(&entry, "bytes")?;
            match alg {
                CoseAlg::Ed25519 => {
                    let public: [u8; 32] = bytes
                        .as_slice()
                        .try_into()
                        .map_err(|_| misuse("an Ed25519 server key is 32 bytes"))?;
                    CoseVerifyKey::ed25519(&public)
                }
                CoseAlg::Esp256 => CoseVerifyKey::p256_sec1(&bytes),
                _ => CoseVerifyKey::hmac(alg, bytes),
            }
            .map_err(super::error::from_error)
        })
        .collect()
}

pub(super) fn contract_sha(value: &JsValue) -> Result<[u8; 32], Fail> {
    bytes_value(value, "contractSha")?
        .try_into()
        .map_err(|_| misuse("`contractSha` is 32 bytes"))
}

/// `{ method, route, pathParams, query?, contractSha, idempotencyKey?,
/// ifMatch? }`, addressed to `audience`.
pub(super) fn binding(binding: &JsValue, audience: &str) -> Result<CallBinding, Fail> {
    let params = field(binding, "pathParams")?;
    if !Array::is_array(&params) {
        return Err(misuse("`pathParams` must be an array of strings"));
    }
    let path_params = Array::from(&params)
        .iter()
        .map(|value| {
            value
                .as_string()
                .ok_or_else(|| misuse("`pathParams` must be an array of strings"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(CallBinding {
        audience: audience.to_owned(),
        method: string(binding, "method")?,
        route: string(binding, "route")?,
        path_params,
        query: opt_string(binding, "query")?,
        contract_sha: contract_sha(&field(binding, "contractSha")?)?,
        idempotency_key: opt_string(binding, "idempotencyKey")?,
        if_match: opt_string(binding, "ifMatch")?,
    })
}

/// `{ fixedIat?, fixedCti? }`, for tests that must reproduce the shared
/// vectors. `None` when no options were given.
pub(super) fn seal_options(
    options: &JsValue,
) -> Result<Option<(Option<i64>, Option<Vec<u8>>)>, Fail> {
    if options.is_undefined() || options.is_null() {
        return Ok(None);
    }
    let iat = field(options, "fixedIat")?;
    let iat = if iat.is_undefined() || iat.is_null() {
        None
    } else {
        // Integral and within JS's safe range, or it is not a clock reading.
        let seconds = iat
            .as_f64()
            .filter(|seconds| seconds.fract() == 0.0 && seconds.abs() < 9_007_199_254_740_992.0)
            .ok_or_else(|| misuse("`fixedIat` must be an integer"))?;
        Some(seconds as i64)
    };
    let cti = field(options, "fixedCti")?;
    let cti = if cti.is_undefined() || cti.is_null() {
        None
    } else {
        Some(bytes_value(&cti, "fixedCti")?)
    };
    Ok(Some((iat, cti)))
}
