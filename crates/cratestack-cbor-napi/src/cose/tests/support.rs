//! The vectors as the pure binding's inputs, and an envelope per key.

use serde_json::Value;

use super::super::envelope::Envelope;
use super::super::fail::Fail;
use super::super::spec::{BindingSpec, SealOptions, ServerKey};

pub(super) const UNARY: &str = include_str!("../../../../cratestack-cose/tests/vectors/unary.json");
pub(super) const KEYS: &str = include_str!("../../../../cratestack-cose/tests/vectors/keys.json");

pub(super) fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex"))
        .collect()
}

pub(super) fn keys() -> Value {
    KEYS.parse().unwrap()
}

pub(super) fn unary() -> Value {
    UNARY.parse().unwrap()
}

pub(super) fn hex_field(value: &Value, field: &str) -> Vec<u8> {
    unhex(value[field].as_str().unwrap_or_else(|| panic!("{field}")))
}

/// The binding of a vector's `binding`.
pub(super) fn binding(binding: &Value) -> BindingSpec {
    let text = |value: &Value| value.as_str().map(str::to_owned);
    BindingSpec {
        method: text(&binding["method"]).unwrap(),
        route: text(&binding["route"]).unwrap(),
        path_params: binding["path_params"]
            .as_array()
            .unwrap()
            .iter()
            .map(|param| text(param).unwrap())
            .collect(),
        query: text(&binding["query"]),
        contract_sha: hex_field(binding, "contract_sha"),
        idempotency_key: text(&binding["bound_headers"]["idempotency_key"]),
        if_match: text(&binding["bound_headers"]["if_match"]),
        payload_type: None,
        payload_accept: None,
    }
}

pub(super) fn alg_name(entry: &Value) -> &'static str {
    match entry["alg"].as_i64().unwrap() {
        -19 => "ed25519",
        -9 => "esp256",
        4 => "hmac256-64",
        _ => "hmac256-256",
    }
}

/// The key as a server key a client trusts: public half, or the secret.
pub(super) fn server_keys(keys: &Value, name: &str) -> Vec<ServerKey> {
    let entry = &keys[name];
    let field = ["public", "public_sec1_uncompressed", "secret"]
        .into_iter()
        .find(|field| entry[field].is_string())
        .unwrap();
    vec![ServerKey {
        alg: alg_name(entry).to_owned(),
        bytes: hex_field(entry, field),
    }]
}

pub(super) fn options(pin: Option<(u64, &str)>) -> SealOptions {
    pin.map_or_else(SealOptions::default, |(iat, cti)| SealOptions {
        fixed_iat: Some(iat as i64),
        fixed_cti: Some(unhex(cti)),
    })
}

pub(super) const AUDIENCE: &str = "payments";

/// A client signing as `key` (in memory), trusting it, pinned when `pin`.
pub(super) fn envelope(
    keys: &Value,
    key: &str,
    pin: Option<(u64, &str)>,
) -> Result<Envelope, Fail> {
    envelope_for(keys, key, AUDIENCE, pin)
}

pub(super) fn envelope_for(
    keys: &Value,
    key: &str,
    audience: &str,
    pin: Option<(u64, &str)>,
) -> Result<Envelope, Fail> {
    let entry = &keys[key];
    let trusted = server_keys(keys, key);
    let options = options(pin);
    match key {
        "p256" => Envelope::p256_scalar(&hex_field(entry, "scalar"), &trusted, audience, options),
        "ed25519" => Envelope::ed25519_seed(&hex_field(entry, "seed"), &trusted, audience, options),
        _ => Envelope::hmac(
            alg_name(entry),
            &hex_field(entry, "secret"),
            &trusted,
            audience,
            options,
        ),
    }
}

pub(super) fn cases(unary: &Value, direction: &str) -> Vec<Value> {
    unary["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["direction"] == direction)
        .cloned()
        .collect()
}

/// The request a response vector answers.
pub(super) fn request_of(unary: &Value, response: &Value) -> Option<Vec<u8>> {
    let of = response["request_digest_of"].as_str()?;
    let request = unary["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == of)
        .unwrap();
    Some(hex_field(request, "cose"))
}
