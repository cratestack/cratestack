//! The external signer: a JS callback signs, and the bytes are the ones the
//! in-memory key makes (issue #1010).

use std::sync::Arc;

use cratestack_cose::{CoseSigner, Ed25519Signer, P256Signer};
use js_sys::{Function, Promise, Uint8Array};
use serde_json::Value;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;
use wasm_bindgen_test::wasm_bindgen_test;

use super::super::ClientEnvelope;
use super::support::*;

/// `(toBeSigned) => Promise<Uint8Array>` over an in-memory signer, counting
/// the calls it gets.
fn callback<S: CoseSigner + 'static>(
    signer: S,
    calls: Arc<std::sync::atomic::AtomicUsize>,
) -> Function {
    let signer = Arc::new(signer);
    Closure::<dyn Fn(Uint8Array) -> Promise>::new(move |tbs: Uint8Array| {
        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let signer = signer.clone();
        future_to_promise(async move {
            let signature = signer
                .sign(&tbs.to_vec())
                .await
                .map_err(|e| e.to_string())?;
            Ok(Uint8Array::from(signature.as_slice()).into())
        })
    })
    .into_js_value()
    .unchecked_into()
}

fn external(keys: &Value, key: &str, sign: Function, pin: Option<(u64, &str)>) -> ClientEnvelope {
    let entry = &keys[key];
    let public = unhex(
        entry["public"]
            .as_str()
            .or(entry["public_sec1_uncompressed"].as_str())
            .unwrap(),
    );
    let alg = if key == "p256" { "esp256" } else { "ed25519" };
    let options = pin.map_or(JsValue::UNDEFINED, |(iat, cti)| options(iat, cti));
    ClientEnvelope::external(
        alg,
        &public,
        sign,
        &server_keys(keys, key),
        "payments",
        &options,
    )
    .map_err(|e| format!("{e:?}"))
    .expect("external envelope")
}

#[wasm_bindgen_test]
async fn an_external_signer_seals_every_sign1_vector_byte_for_byte() {
    let (keys, unary): (Value, Value) = (KEYS.parse().unwrap(), UNARY.parse().unwrap());
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut sealed_count = 0;
    for case in cases(&unary, "request") {
        let key = case["key"].as_str().unwrap();
        let sign = match key {
            "p256" => callback(
                P256Signer::from_scalar(
                    &unhex(keys[key]["scalar"].as_str().unwrap())
                        .try_into()
                        .unwrap(),
                )
                .unwrap(),
                calls.clone(),
            ),
            "ed25519" => callback(
                Ed25519Signer::from_seed(
                    &unhex(keys[key]["seed"].as_str().unwrap())
                        .try_into()
                        .unwrap(),
                ),
                calls.clone(),
            ),
            _ => continue,
        };
        let pin = (case["iat"].as_u64().unwrap(), case["cti"].as_str().unwrap());
        let sealed = settle(external(&keys, key, sign, Some(pin)).seal_request(
            &unhex(case["payload"].as_str().unwrap()),
            &binding(&case["binding"]),
        ))
        .await
        .unwrap_or_else(|e| panic!("{}: {e:?}", case["name"]));
        let sealed = sealed.dyn_into::<Uint8Array>().unwrap().to_vec();
        assert_eq!(
            sealed,
            unhex(case["cose"].as_str().unwrap()),
            "{}",
            case["name"]
        );
        sealed_count += 1;
    }
    assert!(sealed_count > 0);
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        sealed_count
    );
}

#[wasm_bindgen_test]
async fn a_wrong_unthrown_or_malformed_signature_is_refused_and_never_sent() {
    let keys: Value = KEYS.parse().unwrap();
    let unary: Value = UNARY.parse().unwrap();
    let case = cases(&unary, "request")
        .into_iter()
        .find(|case| case["key"] == "ed25519")
        .unwrap();
    let seal = |sign: Function| {
        let envelope = external(&keys, "ed25519", sign, None);
        envelope.seal_request(
            &unhex(case["payload"].as_str().unwrap()),
            &binding(&case["binding"]),
        )
    };
    let returns = |body: &str| -> Function { Function::new_no_args(body) };
    for (name, sign) in [
        (
            "another key's signature",
            callback(Ed25519Signer::from_seed(&[9; 32]), Default::default()),
        ),
        ("a throw", returns("throw new Error('secret detail')")),
        (
            "a rejection",
            returns("return Promise.reject(new Error('secret detail'))"),
        ),
        ("a string", returns("return 'nope'")),
        ("a short signature", returns("return new Uint8Array(63)")),
    ] {
        let error = settle(seal(sign)).await.expect_err(name);
        assert_eq!(
            get(&error, "code").as_string().as_deref(),
            Some("misuse"),
            "{name}"
        );
        // The page's own error text is not echoed.
        assert!(
            !get(&error, "message")
                .as_string()
                .unwrap()
                .contains("secret detail"),
            "{name}"
        );
    }
}

#[wasm_bindgen_test]
fn an_external_signer_of_another_algorithm_or_a_short_ed25519_key_is_misuse() {
    let keys: Value = KEYS.parse().unwrap();
    let trusted = server_keys(&keys, "ed25519");
    let noop = Function::new_no_args("return new Uint8Array(64)");
    for (alg, public) in [
        ("hmac256-64", vec![0u8; 32]),
        ("ed25519", vec![0u8; 31]),
        ("rsa", vec![]),
    ] {
        let error = ClientEnvelope::external(
            alg,
            &public,
            noop.clone(),
            &trusted,
            "a",
            &JsValue::UNDEFINED,
        )
        .err()
        .expect(alg);
        assert_eq!(
            get(&error, "code").as_string().as_deref(),
            Some("misuse"),
            "{alg}"
        );
    }
}
