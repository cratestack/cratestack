//! Misuse, and what the envelope says about itself.

use serde_json::Value;
use wasm_bindgen::JsValue;
use wasm_bindgen_test::wasm_bindgen_test;

use super::super::{ClientEnvelope, contract_header_value};
use super::support::*;

#[wasm_bindgen_test]
fn misuse_is_named_and_the_getters_describe_the_envelope() {
    let keys: Value = KEYS.parse().unwrap();
    let trusted = server_keys(&keys, "ed25519");
    let code = |result: Result<ClientEnvelope, JsValue>| {
        let error = result.err().expect("misuse");
        get(&error, "code").as_string()
    };
    assert_eq!(
        code(ClientEnvelope::ed25519_seed(
            &[0; 31],
            &trusted,
            "a",
            &JsValue::UNDEFINED
        ))
        .as_deref(),
        Some("misuse")
    );
    assert_eq!(
        code(ClientEnvelope::ed25519_seed(
            &[0; 32],
            &trusted,
            "",
            &JsValue::UNDEFINED
        ))
        .as_deref(),
        Some("misuse")
    );
    assert_eq!(
        code(ClientEnvelope::hmac(
            "hmac256-64",
            &[1; 8],
            &trusted,
            "a",
            &JsValue::UNDEFINED
        ))
        .as_deref(),
        Some("misuse")
    );
    assert_eq!(
        code(ClientEnvelope::hmac(
            "nope",
            &[1; 32],
            &trusted,
            "a",
            &JsValue::UNDEFINED
        ))
        .as_deref(),
        Some("misuse")
    );

    let sign1 = envelope(&keys, "ed25519", None);
    assert_eq!(sign1.mode(), "sign1");
    assert_eq!(
        sign1.media_type(),
        r#"application/cose; cose-type="cose-sign1""#
    );
    assert_eq!(sign1.kid(), unhex(keys["ed25519"]["kid"].as_str().unwrap()));
    let mac0 = envelope(&keys, "hmac-256-64", None);
    assert_eq!(mac0.mode(), "mac0");
    assert_eq!(
        mac0.media_type(),
        r#"application/cose; cose-type="cose-mac0""#
    );
}

#[wasm_bindgen_test]
fn the_contract_header_value_is_the_unbound_selector() {
    let digest: Vec<u8> = (0..32).collect();
    assert_eq!(contract_header_value(&digest).unwrap(), "AAECAwQFBgc");
    assert!(contract_header_value(&digest[..5]).is_err());
}
