//! The shared COSE vectors (`cratestack-cose/tests/vectors/*.json`, read in
//! place) driven through the Flutter-facing API, the way the Dart package
//! drives them through the bridge (cratestack#1026).
//!
//! Requests: every case must seal byte for byte at the vector's fixed
//! `iat` and `cti`. ESP256 goes through `from_signer` with the crate's
//! RFC 6979 `P256Signer` (the bridge has no ESP256 constructor yet), which
//! is the signer the vector bytes were made with. Responses: the 12 that
//! answer a signed request open; the 4 that answer an unsigned one are not
//! this client's (it is Required-only, like the Rust client) and are
//! counted, not opened.

#![cfg(feature = "cose")]

use std::path::PathBuf;
use std::sync::Arc;

use cratestack_client_flutter::cose::{
    FlutterCallBinding, FlutterClientEnvelope, FlutterCoseAlg, FlutterCoseError,
    FlutterCoseErrorKind, FlutterCoseMode, FlutterSealOptions, FlutterServerKey,
    cose_contract_header_value,
};
use cratestack_cose::P256Signer;
use serde_json::Value;

fn vectors(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../cratestack-cose/tests/vectors")
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(path).expect("vector file")).expect("json")
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex"))
        .collect()
}

fn text(value: &Value, field: &str) -> String {
    value[field].as_str().expect(field).to_owned()
}

fn call(binding: &Value) -> FlutterCallBinding {
    let opt = |value: &Value| value.as_str().map(str::to_owned);
    FlutterCallBinding {
        method: text(binding, "method"),
        route: text(binding, "route"),
        path_params: binding["path_params"]
            .as_array()
            .expect("path_params")
            .iter()
            .map(|value| value.as_str().expect("tstr").to_owned())
            .collect(),
        query: opt(&binding["query"]),
        contract_sha: unhex(binding["contract_sha"].as_str().expect("sha")),
        idempotency_key: opt(&binding["bound_headers"]["idempotency_key"]),
        if_match: opt(&binding["bound_headers"]["if_match"]),
    }
}

fn alg(id: i64) -> FlutterCoseAlg {
    match id {
        -19 => FlutterCoseAlg::Ed25519,
        -9 => FlutterCoseAlg::Esp256,
        4 => FlutterCoseAlg::Hmac256_64,
        5 => FlutterCoseAlg::Hmac256_256,
        other => panic!("alg {other}"),
    }
}

/// The server key a response from `key` verifies with.
fn server_key(keys: &Value, name: &str) -> FlutterServerKey {
    let entry = &keys[name];
    let field = ["public", "public_sec1_uncompressed", "secret"]
        .into_iter()
        .find_map(|field| entry[field].as_str())
        .expect("key material");
    FlutterServerKey {
        alg: alg(entry["alg"].as_i64().expect("alg")),
        bytes: unhex(field),
    }
}

/// A client envelope signing as `key`, pinned to `iat`/`cti` when given,
/// trusting the server key `server`.
fn envelope(
    keys: &Value,
    key: &str,
    server: &str,
    pin: Option<(i64, &str)>,
) -> FlutterClientEnvelope {
    let options = pin.map(|(iat, cti)| FlutterSealOptions {
        fixed_iat: Some(iat),
        fixed_cti: Some(unhex(cti)),
    });
    let server_keys = vec![server_key(keys, server)];
    let audience = "payments".to_owned();
    let entry = &keys[key];
    match key {
        "p256" => {
            let scalar: [u8; 32] = unhex(entry["scalar"].as_str().unwrap()).try_into().unwrap();
            let signer = Arc::new(P256Signer::from_scalar(&scalar).expect("scalar"));
            FlutterClientEnvelope::from_signer(signer, server_keys, audience, options)
        }
        "ed25519" => FlutterClientEnvelope::ed25519_seed(
            unhex(entry["seed"].as_str().unwrap()),
            server_keys,
            audience,
            options,
        ),
        _ => FlutterClientEnvelope::hmac(
            alg(entry["alg"].as_i64().unwrap()),
            unhex(entry["secret"].as_str().unwrap()),
            server_keys,
            audience,
            options,
        ),
    }
    .expect("envelope")
}

#[tokio::test]
async fn every_request_vector_seals_byte_for_byte() {
    let keys = vectors("keys.json");
    let unary = vectors("unary.json");
    let mut sealed = 0;
    for case in unary["cases"].as_array().unwrap().iter() {
        if case["direction"] != "request" {
            continue;
        }
        let iat = i64::try_from(case["iat"].as_u64().unwrap()).unwrap();
        let name = text(case, "name");
        let key = text(case, "key");
        let envelope = envelope(
            &keys,
            &key,
            &key,
            Some((iat, case["cti"].as_str().unwrap())),
        );
        let got = envelope
            .seal_request(
                unhex(case["payload"].as_str().unwrap()),
                call(&case["binding"]),
            )
            .await
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(got, unhex(case["cose"].as_str().unwrap()), "{name}");
        assert_eq!(
            envelope.kid(),
            unhex(keys[&key]["kid"].as_str().unwrap()),
            "{name}"
        );
        sealed += 1;
    }
    // 16 (RPC and REST, four algorithms, two `cti` shapes) and the
    // empty-query twin of the first.
    assert_eq!(sealed, 17);
}

fn responses(unary: &Value) -> Vec<&Value> {
    unary["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["direction"] == "response")
        .collect()
}

/// The sealed request a response answers: the linked case's `cose` bytes.
fn linked_request(unary: &Value, response: &Value) -> Option<Vec<u8>> {
    let of = response["request_digest_of"].as_str()?;
    let request = unary["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == of)?;
    Some(unhex(request["cose"].as_str().unwrap()))
}

#[tokio::test]
async fn the_signed_request_responses_open_and_the_unsigned_ones_are_not_this_clients() {
    let keys = vectors("keys.json");
    let unary = vectors("unary.json");
    let (mut opened, mut skipped) = (0, 0);
    for case in responses(&unary) {
        let name = text(case, "name");
        let Some(request) = linked_request(&unary, case) else {
            skipped += 1;
            continue;
        };
        let key = text(case, "key");
        let result = envelope(&keys, &key, &key, None)
            .open_response(
                unhex(case["cose"].as_str().unwrap()),
                call(&case["binding"]),
                request,
                u16::try_from(case["binding"]["status"].as_u64().unwrap()).unwrap(),
            )
            .await
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(
            result.payload,
            unhex(case["payload"].as_str().unwrap()),
            "{name}"
        );
        assert_eq!(
            result.kid,
            unhex(keys[&key]["kid"].as_str().unwrap()),
            "{name}"
        );
        assert_eq!(
            result.thumbprint,
            unhex(keys[&key]["thumbprint"].as_str().unwrap())
        );
        assert_eq!(result.alg, alg(case["alg_id"].as_i64().unwrap()), "{name}");
        opened += 1;
    }
    assert_eq!((opened, skipped), (12, 4));
}

#[tokio::test]
async fn a_tampered_response_is_rejected_and_says_nothing_else() {
    let keys = vectors("keys.json");
    let unary = vectors("unary.json");
    for case in responses(&unary) {
        let Some(request) = linked_request(&unary, case) else {
            continue;
        };
        let name = text(case, "name");
        let key = text(case, "key");
        let status = u16::try_from(case["binding"]["status"].as_u64().unwrap()).unwrap();
        let body = unhex(case["cose"].as_str().unwrap());
        let client = envelope(&keys, &key, &key, None);
        let open = |body: Vec<u8>, binding: FlutterCallBinding, request: Vec<u8>, status: u16| {
            client.open_response(body, binding, request, status)
        };
        let mut attempts = Vec::new();
        // One flipped bit at the start, the middle and the end.
        for at in [0, body.len() / 2, body.len() - 1] {
            let mut tampered = body.clone();
            tampered[at] ^= 0x01;
            attempts.push(open(
                tampered,
                call(&case["binding"]),
                request.clone(),
                status,
            ));
        }
        let mut other_request = request.clone();
        other_request[request.len() - 1] ^= 0x01;
        attempts.push(open(
            body.clone(),
            call(&case["binding"]),
            other_request,
            status,
        ));
        attempts.push(open(
            body.clone(),
            call(&case["binding"]),
            request.clone(),
            status ^ 1,
        ));
        let mut other_route = call(&case["binding"]);
        other_route.route.push('x');
        attempts.push(open(body.clone(), other_route, request.clone(), status));
        for attempt in attempts {
            let error = attempt.await.expect_err(&name);
            assert_eq!(error, FlutterCoseError::rejected(), "{name}");
        }
    }
}

#[tokio::test]
async fn a_response_of_the_other_mode_or_key_is_rejected() {
    let keys = vectors("keys.json");
    let unary = vectors("unary.json");
    let case = responses(&unary)[0]; // rpc-response-sign1-ed25519
    let request = linked_request(&unary, case).unwrap();
    let body = unhex(case["cose"].as_str().unwrap());
    for (key, server) in [("ed25519", "ed25519_other"), ("hmac-256-64", "hmac-256-64")] {
        let error = envelope(&keys, key, server, None)
            .open_response(body.clone(), call(&case["binding"]), request.clone(), 200)
            .await
            .expect_err(key);
        assert_eq!(error, FlutterCoseError::rejected(), "{key}");
    }
}

#[test]
fn misuse_is_named_and_the_getters_describe_the_envelope() {
    let keys = vectors("keys.json");
    let server = vec![server_key(&keys, "ed25519")];
    let seed = unhex(keys["ed25519"]["seed"].as_str().unwrap());
    let misuse = |result: Result<FlutterClientEnvelope, FlutterCoseError>| match result {
        Err(error) => assert_eq!(error.kind, FlutterCoseErrorKind::Misuse, "{error}"),
        Ok(_) => panic!("expected misuse"),
    };
    misuse(FlutterClientEnvelope::ed25519_seed(
        vec![0; 31],
        server.clone(),
        "a".into(),
        None,
    ));
    misuse(FlutterClientEnvelope::ed25519_seed(
        seed.clone(),
        server.clone(),
        String::new(),
        None,
    ));
    misuse(FlutterClientEnvelope::hmac(
        FlutterCoseAlg::Hmac256_64,
        vec![1; 8],
        server.clone(),
        "a".into(),
        None,
    ));
    misuse(FlutterClientEnvelope::hmac(
        FlutterCoseAlg::Ed25519,
        vec![1; 32],
        server.clone(),
        "a".into(),
        None,
    ));
    let bad_key = FlutterServerKey {
        alg: FlutterCoseAlg::Ed25519,
        bytes: vec![0; 5],
    };
    misuse(FlutterClientEnvelope::ed25519_seed(
        seed,
        vec![bad_key],
        "a".into(),
        None,
    ));

    let sign1 = envelope(&keys, "ed25519", "ed25519", None);
    assert_eq!(sign1.mode(), FlutterCoseMode::Sign1);
    assert_eq!(
        sign1.media_type(),
        r#"application/cose; cose-type="cose-sign1""#
    );
    let mac0 = envelope(&keys, "hmac-256-64", "hmac-256-64", None);
    assert_eq!(mac0.mode(), FlutterCoseMode::Mac0);
    assert_eq!(
        mac0.media_type(),
        r#"application/cose; cose-type="cose-mac0""#
    );
}

#[test]
fn the_contract_header_value_is_the_unbound_selector() {
    let digest: Vec<u8> = (0..32).collect();
    assert_eq!(
        cose_contract_header_value(digest.clone()).unwrap(),
        "AAECAwQFBgc"
    );
    assert!(cose_contract_header_value(digest[..5].to_vec()).is_err());
}

#[test]
fn a_server_key_never_prints_its_bytes() {
    // For a Mac0 server the bytes are the shared secret.
    let key = FlutterServerKey {
        alg: FlutterCoseAlg::Hmac256_64,
        bytes: vec![0xAB; 40],
    };
    let shown = format!("{key:?}");
    assert!(shown.contains("<40 bytes>"), "{shown}");
    assert!(
        !shown.contains("171") && !shown.to_lowercase().contains("ab,"),
        "{shown}"
    );
}
