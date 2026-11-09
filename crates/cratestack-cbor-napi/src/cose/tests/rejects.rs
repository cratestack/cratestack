//! What opening refuses, and what misuse says (issue #1010). Every refusal
//! of a received response is the same `rejected` with no message.

use super::super::spec::BindingSpec;
use super::support::*;

/// The first signed response vector of `key`, its request, and its binding.
fn response_of(unary: &serde_json::Value, key: &str) -> (serde_json::Value, Vec<u8>, BindingSpec) {
    let case = cases(unary, "response")
        .into_iter()
        .find(|case| case["key"] == key && case["request_digest_of"].is_string())
        .unwrap();
    let request = request_of(unary, &case).unwrap();
    let bound = binding(&case["binding"]);
    (case, request, bound)
}

fn status(case: &serde_json::Value) -> u16 {
    case["binding"]["status"].as_u64().unwrap() as u16
}

fn rejected<T>(result: Result<T, super::super::fail::Fail>, why: &str) {
    let error = result
        .err()
        .unwrap_or_else(|| panic!("{why} must be refused"));
    assert_eq!(
        (error.code, error.message.as_str()),
        ("rejected", ""),
        "{why}"
    );
}

#[test]
fn a_wrong_audience_a_replay_and_another_signer_all_reject() {
    let (keys, unary) = (keys(), unary());
    for key in ["ed25519", "p256", "hmac-256-64"] {
        let (case, request, bound) = response_of(&unary, key);
        let (body, status) = (hex_field(&case, "cose"), status(&case));
        let open = |client: &super::super::envelope::Envelope, request: &[u8]| {
            client.open_response(&body, &bound, request, status, None)
        };
        // Control: the right client, request and audience open it.
        let pinned = envelope(&keys, key, None).unwrap();
        assert!(open(&pinned, &request).is_ok(), "{key}");

        let wrong_audience = envelope_for(&keys, key, "someone-else", None).unwrap();
        rejected(open(&wrong_audience, &request), "a wrong audience");

        // The answer to another request (a replay of a recorded response).
        let other = cases(&unary, "request")
            .into_iter()
            .find(|c| {
                c["key"] == case_key_of_request(&unary, &case)
                    && c["name"] != case["request_digest_of"]
            })
            .unwrap();
        rejected(
            open(&pinned, &hex_field(&other, "cose")),
            "a replayed response",
        );
        rejected(open(&pinned, &[]), "a response to no request");
    }
    // A response signed by a key the client does not trust.
    let (case, request, bound) = response_of(&unary, "ed25519");
    let stranger = {
        let mut keys = keys.clone();
        keys["ed25519"]["public"] = keys["ed25519_other"]["public"].clone();
        envelope(&keys, "ed25519", None).unwrap()
    };
    rejected(
        stranger.open_response(
            &hex_field(&case, "cose"),
            &bound,
            &request,
            status(&case),
            None,
        ),
        "an untrusted signer",
    );
}

fn case_key_of_request(
    unary: &serde_json::Value,
    response: &serde_json::Value,
) -> serde_json::Value {
    let of = response["request_digest_of"].as_str().unwrap();
    unary["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == of)
        .unwrap()["key"]
        .clone()
}

#[test]
fn a_payload_type_that_was_not_asked_for_is_refused() {
    let (keys, unary) = (keys(), unary());
    let (case, request, bound) = response_of(&unary, "ed25519");
    let client = envelope(&keys, "ed25519", None).unwrap();
    let (body, status) = (hex_field(&case, "cose"), status(&case));
    // The vector asked for CBOR; the server cannot answer under a compact type.
    let compact = Some("application/vnd.cratestack.compact+cbor");
    rejected(
        client.open_response(&body, &bound, &request, status, compact),
        "an unrequested answer type",
    );
}

#[test]
fn misuse_is_named_and_the_getters_describe_the_envelope() {
    let (keys, unary) = (keys(), unary());
    let ed = server_keys(&keys, "ed25519");
    let envelope = |result: Result<super::super::envelope::Envelope, _>| {
        result.err().map(|e: super::super::fail::Fail| e.code)
    };
    use super::super::envelope::Envelope;
    assert_eq!(
        envelope(Envelope::ed25519_seed(&[0; 31], &ed, "a", options(None))),
        Some("misuse")
    );
    assert_eq!(
        envelope(Envelope::ed25519_seed(&[0; 32], &ed, "", options(None))),
        Some("misuse")
    );
    assert_eq!(
        envelope(Envelope::hmac(
            "hmac256-64",
            &[1; 8],
            &ed,
            "a",
            options(None)
        )),
        Some("misuse")
    );
    assert_eq!(
        envelope(Envelope::hmac("nope", &[1; 32], &ed, "a", options(None))),
        Some("misuse")
    );

    let case = &cases(&unary, "request")[0];
    let client = super::support::envelope(&keys, "ed25519", None).unwrap();
    assert_eq!(
        (client.mode(), client.media_type()),
        ("sign1", "application/cose; cose-type=\"cose-sign1\"")
    );
    let mac = super::support::envelope(&keys, "hmac-256-64", None).unwrap();
    assert_eq!(mac.mode(), "mac0");

    // A short contract digest, an unsealable payload type, a bad accept list.
    let payload = hex_field(case, "payload");
    let mut bound = binding(&case["binding"]);
    bound.contract_sha.pop();
    assert_eq!(
        client.seal_request(&payload, &bound).err().unwrap().code,
        "misuse"
    );
    let mut bound = binding(&case["binding"]);
    bound.payload_type = Some("application/cose".to_owned());
    assert_eq!(
        client.seal_request(&payload, &bound).err().unwrap().code,
        "misuse"
    );
    let mut bound = binding(&case["binding"]);
    bound.payload_accept = Some(vec!["multipart/form-data".to_owned()]);
    assert_eq!(
        client.selector_headers(&bound).err().unwrap().code,
        "misuse"
    );

    // The selector headers of a compact call, and of a default one.
    let mut bound = binding(&case["binding"]);
    assert!(client.selector_headers(&bound).unwrap().is_empty());
    bound.payload_type = Some("application/vnd.cratestack.compact+cbor".to_owned());
    bound.payload_accept = Some(vec![
        "application/vnd.cratestack.compact+cbor".to_owned(),
        "application/cbor".to_owned(),
    ]);
    let headers = client.selector_headers(&bound).unwrap();
    let names: Vec<_> = headers
        .iter()
        .map(|(name, _)| name.to_ascii_lowercase())
        .collect();
    assert_eq!(
        names,
        ["cratestack-payload-type", "cratestack-payload-accept"]
    );
}
