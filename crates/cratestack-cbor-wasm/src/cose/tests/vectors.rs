//! The shared vectors through `ClientEnvelope`.

use js_sys::Uint8Array;
use serde_json::Value;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::wasm_bindgen_test;

use super::support::*;

#[wasm_bindgen_test]
async fn every_request_vector_seals_byte_for_byte() {
    let (keys, unary): (Value, Value) = (KEYS.parse().unwrap(), UNARY.parse().unwrap());
    let requests = cases(&unary, "request");
    for case in &requests {
        let key = case["key"].as_str().unwrap();
        let pin = (case["iat"].as_u64().unwrap(), case["cti"].as_str().unwrap());
        let sealed = settle(envelope(&keys, key, Some(pin)).seal_request(
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
    }
    assert_eq!(requests.len(), 17);
}

#[wasm_bindgen_test]
async fn the_signed_request_responses_open_and_tampered_ones_reject() {
    let (keys, unary): (Value, Value) = (KEYS.parse().unwrap(), UNARY.parse().unwrap());
    let all = unary["cases"].as_array().unwrap();
    let (mut opened, mut skipped) = (0, 0);
    for case in cases(&unary, "response") {
        let Some(of) = case["request_digest_of"].as_str() else {
            skipped += 1;
            continue;
        };
        let request = unhex(
            all.iter().find(|c| c["name"] == of).unwrap()["cose"]
                .as_str()
                .unwrap(),
        );
        let key = case["key"].as_str().unwrap();
        let status = case["binding"]["status"].as_u64().unwrap() as u16;
        let body = unhex(case["cose"].as_str().unwrap());
        let client = envelope(&keys, key, None);
        let open = |body: &[u8], request: &[u8], status: u16| {
            client.open_response(body, &binding(&case["binding"]), request, status)
        };
        let result = settle(open(&body, &request, status))
            .await
            .unwrap_or_else(|e| panic!("{}: {e:?}", case["name"]));
        let payload = get(&result, "payload")
            .dyn_into::<Uint8Array>()
            .unwrap()
            .to_vec();
        assert_eq!(payload, unhex(case["payload"].as_str().unwrap()));
        let kid = get(&result, "kid")
            .dyn_into::<Uint8Array>()
            .unwrap()
            .to_vec();
        assert_eq!(kid, unhex(keys[key]["kid"].as_str().unwrap()));
        assert!(get(&result, "alg").as_string().is_some());
        assert_eq!(
            get(&result, "thumbprint")
                .dyn_into::<Uint8Array>()
                .unwrap()
                .length(),
            32
        );
        opened += 1;

        let mut flipped = body.clone();
        flipped[body.len() / 2] ^= 1;
        for attempt in [
            open(&flipped, &request, status),
            open(&body, &request, status ^ 1),
        ] {
            let error = settle(attempt).await.expect_err("tampered");
            assert_eq!(get(&error, "code").as_string().as_deref(), Some("rejected"));
            assert_eq!(get(&error, "message").as_string().as_deref(), Some(""));
        }
    }
    assert_eq!((opened, skipped), (12, 4));
}
