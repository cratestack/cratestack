//! The vectors as JS values, and an envelope per key.

use std::sync::Arc;

use cratestack_cose::P256Signer;
use js_sys::{Array, Object, Reflect, Uint8Array};
use serde::Serialize;
use serde_json::{Value, json};
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

use super::super::ClientEnvelope;

pub(super) const UNARY: &str = include_str!("../../../../cratestack-cose/tests/vectors/unary.json");
pub(super) const KEYS: &str = include_str!("../../../../cratestack-cose/tests/vectors/keys.json");

pub(super) fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex"))
        .collect()
}

pub(super) fn js(value: &Value) -> JsValue {
    let serializer = serde_wasm_bindgen::Serializer::new()
        .serialize_maps_as_objects(true)
        .serialize_missing_as_null(true);
    value.serialize(&serializer).expect("to JS")
}

pub(super) fn set(object: &JsValue, name: &str, value: impl Into<JsValue>) {
    Reflect::set(object, &name.into(), &value.into()).expect("set");
}

pub(super) fn get(object: &JsValue, name: &str) -> JsValue {
    Reflect::get(object, &name.into()).expect("get")
}

pub(super) fn bytes(hex: &str) -> Uint8Array {
    Uint8Array::from(unhex(hex).as_slice())
}

/// The binding object of a vector's `binding`, camelCase as the web API
/// takes it.
pub(super) fn binding(binding: &Value) -> JsValue {
    let object = js(&json!({
        "method": binding["method"],
        "route": binding["route"],
        "pathParams": binding["path_params"],
        "query": binding["query"],
        "idempotencyKey": binding["bound_headers"]["idempotency_key"],
        "ifMatch": binding["bound_headers"]["if_match"],
    }));
    set(
        &object,
        "contractSha",
        bytes(binding["contract_sha"].as_str().unwrap()),
    );
    object
}

pub(super) fn server_keys(keys: &Value, name: &str) -> JsValue {
    let entry = &keys[name];
    let material = ["public", "public_sec1_uncompressed", "secret"]
        .into_iter()
        .find_map(|field| entry[field].as_str())
        .unwrap();
    let alg = match entry["alg"].as_i64().unwrap() {
        -19 => "ed25519",
        -9 => "esp256",
        4 => "hmac256-64",
        _ => "hmac256-256",
    };
    let key = Object::new();
    set(&key, "alg", alg);
    set(&key, "bytes", bytes(material));
    Array::of1(&key).into()
}

pub(super) fn options(iat: u64, cti: &str) -> JsValue {
    let options = Object::new();
    set(&options, "fixedIat", iat as f64);
    set(&options, "fixedCti", bytes(cti));
    options.into()
}

/// A client signing as `key`, trusting `server`, pinned when `pin`.
pub(super) fn envelope(keys: &Value, key: &str, pin: Option<(u64, &str)>) -> ClientEnvelope {
    let entry = &keys[key];
    let trusted = server_keys(keys, key);
    let options = pin.map_or(JsValue::UNDEFINED, |(iat, cti)| options(iat, cti));
    let hex = |field: &str| unhex(entry[field].as_str().unwrap());
    match key {
        "p256" => {
            let scalar: [u8; 32] = hex("scalar").try_into().unwrap();
            let signer = Arc::new(P256Signer::from_scalar(&scalar).unwrap());
            ClientEnvelope::build(signer, &trusted, "payments", &options)
        }
        "ed25519" => ClientEnvelope::ed25519_seed(&hex("seed"), &trusted, "payments", &options),
        _ => {
            let alg = if entry["alg"] == 4 {
                "hmac256-64"
            } else {
                "hmac256-256"
            };
            ClientEnvelope::hmac(alg, &hex("secret"), &trusted, "payments", &options)
        }
    }
    .map_err(|e| format!("{e:?}"))
    .expect("envelope")
}

pub(super) async fn settle(promise: js_sys::Promise) -> Result<JsValue, JsValue> {
    JsFuture::from(promise).await
}

pub(super) fn cases<'a>(unary: &'a Value, direction: &str) -> Vec<&'a Value> {
    let all = unary["cases"].as_array().unwrap().iter();
    all.filter(|case| case["direction"] == direction).collect()
}
