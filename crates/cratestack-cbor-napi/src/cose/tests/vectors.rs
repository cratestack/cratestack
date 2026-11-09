//! The shared vectors through the pure binding (issue #1010).

use super::support::*;

#[test]
fn every_request_vector_seals_byte_for_byte() {
    let (keys, unary) = (keys(), unary());
    let requests = cases(&unary, "request");
    for case in &requests {
        let key = case["key"].as_str().unwrap();
        let pin = (case["iat"].as_u64().unwrap(), case["cti"].as_str().unwrap());
        let sealed = envelope(&keys, key, Some(pin))
            .unwrap()
            .seal_request(&hex_field(case, "payload"), &binding(&case["binding"]))
            .unwrap_or_else(|e| panic!("{}: {e:?}", case["name"]));
        assert_eq!(sealed, hex_field(case, "cose"), "{}", case["name"]);
    }
    assert_eq!(requests.len(), 17);
}

#[test]
fn the_signed_request_responses_open_and_tampered_ones_reject() {
    let (keys, unary) = (keys(), unary());
    let (mut opened, mut skipped) = (0, 0);
    for case in cases(&unary, "response") {
        let Some(request) = request_of(&unary, &case) else {
            skipped += 1;
            continue;
        };
        let key = case["key"].as_str().unwrap();
        let status = case["binding"]["status"].as_u64().unwrap() as u16;
        let body = hex_field(&case, "cose");
        let client = envelope(&keys, key, None).unwrap();
        let bound = binding(&case["binding"]);
        let result = client
            .open_response(&body, &bound, &request, status, None)
            .unwrap_or_else(|e| panic!("{}: {e:?}", case["name"]));
        assert_eq!(result.payload, hex_field(&case, "payload"));
        assert_eq!(result.kid, hex_field(&keys[key], "kid"));
        assert_eq!(result.thumbprint.len(), 32);
        assert_eq!(result.payload_type, "application/cbor");
        opened += 1;

        let mut flipped = body.clone();
        flipped[body.len() / 2] ^= 1;
        for attempt in [
            client.open_response(&flipped, &bound, &request, status, None),
            client.open_response(&body, &bound, &request, status ^ 1, None),
        ] {
            let error = attempt.expect_err("tampered");
            assert_eq!((error.code, error.message.as_str()), ("rejected", ""));
        }
    }
    assert_eq!((opened, skipped), (12, 4));
}
