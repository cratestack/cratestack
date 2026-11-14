//! The keystore signer's tagged outcomes (cratestack#1151): a callback that
//! resolves to `{ outcome, message? }` instead of bytes fails the seal as
//! that kind, per call, and a plain `Uint8Array` callback is unaffected.

use std::cell::Cell;
use std::rc::Rc;

use cratestack_cose::{CoseSigner, P256Signer};
use js_sys::{Function, Object, Promise, Uint8Array};
use serde_json::Value;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;
use wasm_bindgen_test::wasm_bindgen_test;

use super::super::ClientEnvelope;
use super::support::*;

fn tagged(outcome: &str, message: Option<&str>) -> JsValue {
    let object = Object::new();
    set(&object, "outcome", outcome);
    if let Some(message) = message {
        set(&object, "message", message);
    }
    object.into()
}

/// A callback that resolves, after a real suspension, to whatever `answer`
/// returns for its call number (from 0) and the real signature of the bytes
/// it was asked to sign.
fn callback(answer: impl Fn(usize, Vec<u8>) -> JsValue + 'static) -> Function {
    let scalar: [u8; 32] = unhex(
        KEYS.parse::<Value>().unwrap()["p256"]["scalar"]
            .as_str()
            .unwrap(),
    )
    .try_into()
    .unwrap();
    let signer = Rc::new(P256Signer::from_scalar(&scalar).unwrap());
    let calls = Cell::new(0usize);
    Closure::<dyn Fn(Uint8Array) -> Promise>::new(move |tbs: Uint8Array| {
        let n = calls.get();
        calls.set(n + 1);
        let signer = signer.clone();
        let tbs = tbs.to_vec();
        let answer = answer(n, signer_signature(&signer, &tbs));
        future_to_promise(async move {
            // Suspend once, as a keystore prompt does.
            wasm_bindgen_futures::JsFuture::from(Promise::resolve(&JsValue::NULL))
                .await
                .ok();
            Ok(answer)
        })
    })
    .into_js_value()
    .unchecked_into()
}

/// `P256Signer::sign` never suspends, so one poll settles it.
fn signer_signature(signer: &P256Signer, tbs: &[u8]) -> Vec<u8> {
    let mut future = Box::pin(signer.sign(tbs));
    let waker = std::task::Waker::noop();
    match future
        .as_mut()
        .poll(&mut std::task::Context::from_waker(waker))
    {
        std::task::Poll::Ready(signature) => signature.expect("sign"),
        std::task::Poll::Pending => unreachable!("P256Signer does not suspend"),
    }
}

fn envelope(public: &[u8], sign: Function) -> ClientEnvelope {
    let keys: Value = KEYS.parse().unwrap();
    ClientEnvelope::external(
        "esp256",
        public,
        sign,
        &server_keys(&keys, "p256"),
        "payments",
        &JsValue::UNDEFINED,
    )
    .map_err(|e| format!("{e:?}"))
    .expect("external envelope")
}

fn public() -> Vec<u8> {
    let keys: Value = KEYS.parse().unwrap();
    unhex(keys["p256"]["public_sec1_uncompressed"].as_str().unwrap())
}

fn seal(envelope: &ClientEnvelope) -> Promise {
    let unary: Value = UNARY.parse().unwrap();
    let case = cases(&unary, "request")
        .into_iter()
        .find(|case| case["key"] == "p256")
        .unwrap();
    envelope.seal_request(
        &unhex(case["payload"].as_str().unwrap()),
        &binding(&case["binding"]),
    )
}

async fn rejection(envelope: &ClientEnvelope) -> (String, String) {
    let error = settle(seal(envelope)).await.expect_err("must not seal");
    (
        get(&error, "code").as_string().unwrap(),
        get(&error, "message").as_string().unwrap(),
    )
}

#[wasm_bindgen_test]
async fn each_tagged_outcome_rejects_with_its_own_code() {
    for (outcome, message, code, want_message) in [
        ("cancelled", Some("ignored"), "signer_cancelled", ""),
        ("timedOut", Some("ignored"), "signer_timed_out", ""),
        (
            "failed",
            Some("keystore locked"),
            "signer_failed",
            "keystore locked",
        ),
        ("failed", None, "signer_failed", ""),
    ] {
        let envelope = envelope(&public(), callback(move |_, _| tagged(outcome, message)));
        let (got_code, got_message) = rejection(&envelope).await;
        assert_eq!(
            (got_code.as_str(), got_message.as_str()),
            (code, want_message)
        );
    }
}

#[wasm_bindgen_test]
async fn an_unknown_tag_is_misuse_not_a_signer_kind() {
    let envelope = envelope(&public(), callback(|_, _| tagged("exploded", Some("x"))));
    assert_eq!(rejection(&envelope).await.0, "misuse");
}

#[wasm_bindgen_test]
async fn a_failure_does_not_leak_into_the_next_seal() {
    let envelope = envelope(
        &public(),
        callback(|n, signature| {
            if n == 0 {
                tagged("timedOut", None)
            } else {
                Uint8Array::from(signature.as_slice()).into()
            }
        }),
    );
    assert_eq!(rejection(&envelope).await.0, "signer_timed_out");
    settle(seal(&envelope)).await.expect("the next seal signs");
}

#[wasm_bindgen_test]
async fn interleaved_seals_keep_their_outcomes_apart() {
    let envelope = envelope(
        &public(),
        callback(|n, signature| {
            if n == 0 {
                tagged("cancelled", None)
            } else {
                Uint8Array::from(signature.as_slice()).into()
            }
        }),
    );
    // Both are in flight before either settles.
    let (first, second) = (seal(&envelope), seal(&envelope));
    // Whichever the runtime polls first is call 0: exactly one is cancelled,
    // and the other still signs.
    let (first, second) = (settle(first).await, settle(second).await);
    let codes: Vec<_> = [&first, &second]
        .iter()
        .map(|r| r.as_ref().map_err(|e| get(e, "code").as_string().unwrap()))
        .collect();
    assert_eq!(codes.iter().filter(|r| r.is_ok()).count(), 1, "{codes:?}");
    assert!(
        codes
            .iter()
            .any(|r| *r == Err("signer_cancelled".to_owned()))
    );
}

#[wasm_bindgen_test]
async fn an_spki_public_key_is_accepted() {
    let mut spki = unhex("3059301306072a8648ce3d020106082a8648ce3d030107034200");
    spki.extend_from_slice(&public());
    let envelope = envelope(
        &spki,
        callback(|_, signature| Uint8Array::from(signature.as_slice()).into()),
    );
    settle(seal(&envelope)).await.expect("seals");
}
