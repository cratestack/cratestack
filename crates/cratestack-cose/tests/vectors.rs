//! The shared, language-neutral vectors (ADR 0006 P0, §11), and the sizes
//! ADR 0006 §3 measured.
//!
//! `tests/vectors/*.json` hold hex only, so the wasm, napi, TypeScript and
//! Dart bindings can check themselves against the same bytes. This test
//! re-derives every vector from the fixed inputs and compares; it never
//! rewrites the files unless `CRATESTACK_COSE_WRITE_VECTORS=1` is set.
//! Regenerating is not how a mismatch gets fixed: the bytes are also
//! checked against `coset` (`interop_coset.rs`) and the sizes against the
//! hand-computed breakdown below, both independent of the files.

mod common;

use std::path::PathBuf;

use common::fixture::payment_bytes;
use common::{CTI_2, CTI_16, IAT, hex, rest_request, rpc_request};
use cratestack_core::Binding;
use cratestack_cose::{CoseAlg, external_aad};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Case {
    name: String,
    alg_id: i64,
    direction: String,
    binding: BindingJson,
    iat: Option<u64>,
    cti: Option<String>,
    external_aad: String,
    cose: String,
    size: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct BindingJson {
    method: String,
    route: String,
    path_params: Vec<String>,
    query: Option<String>,
    schema_sha: String,
    payload_type: String,
    request_digest: Option<String>,
    status: Option<u16>,
}

fn binding_json(bind: &Binding<'_>) -> BindingJson {
    BindingJson {
        method: bind.method.to_string(),
        route: bind.route.to_string(),
        path_params: bind.path_params.iter().map(str::to_owned).collect(),
        query: bind.query.as_deref().map(str::to_owned),
        schema_sha: hex(&bind.schema_sha),
        payload_type: bind.payload_media_type.to_string(),
        request_digest: bind.request_digest.map(|digest| hex(&digest)),
        status: bind.status,
    }
}

fn alg_name(alg: CoseAlg) -> &'static str {
    match alg {
        CoseAlg::Ed25519 => "sign1-ed25519",
        CoseAlg::Esp256 => "sign1-esp256",
        CoseAlg::Hmac256_64 => "mac0-hmac256-64",
        CoseAlg::Hmac256_256 => "mac0-hmac256-256",
        _ => unreachable!(),
    }
}

/// Derive every case from the fixed inputs, and check each one opens.
async fn derive_cases() -> Vec<Case> {
    let payload = payment_bytes();
    let mut cases = Vec::new();
    for (binding_name, request) in [("rpc", rpc_request()), ("rest", rest_request())] {
        for alg in CoseAlg::ALL {
            let mut first_request = None;
            for (cti_name, cti) in [("cti16", CTI_16), ("cti2", CTI_2)] {
                let sealed = common::client(alg, IAT, cti)
                    .seal_request(&payload, &request)
                    .await
                    .expect("seal");
                let opened = common::server(alg, IAT)
                    .open_request(sealed.clone(), &request)
                    .await
                    .expect("a vector must open");
                assert_eq!(opened.payload.as_ref(), payload.as_slice());
                cases.push(Case {
                    name: format!("{binding_name}-request-{}-{cti_name}", alg_name(alg)),
                    alg_id: alg.id(),
                    direction: "request".to_owned(),
                    binding: binding_json(&request),
                    iat: Some(IAT),
                    cti: Some(cti.to_owned()),
                    external_aad: hex(&external_aad(&request).expect("aad")),
                    cose: hex(&sealed),
                    size: sealed.len(),
                });
                first_request.get_or_insert(sealed);
            }
            let request_body = first_request.expect("sealed a request");
            let response = common::response_to(&request, &request_body, 200);
            let sealed = common::server(alg, IAT)
                .seal_response(&payload, &response)
                .await
                .expect("seal response");
            let opened = common::client(alg, IAT, CTI_16)
                .open_response(sealed.clone(), &response)
                .await
                .expect("a response vector must open");
            assert_eq!(opened.payload.as_ref(), payload.as_slice());
            cases.push(Case {
                name: format!("{binding_name}-response-{}", alg_name(alg)),
                alg_id: alg.id(),
                direction: "response".to_owned(),
                binding: binding_json(&response),
                iat: None,
                cti: None,
                external_aad: hex(&external_aad(&response).expect("aad")),
                cose: hex(&sealed),
                size: sealed.len(),
            });
        }
    }
    cases
}

fn vectors_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/vectors")
}

fn keys_json() -> serde_json::Value {
    let ed = common::ed25519().verify_key();
    let p = common::p256().verify_key();
    let mac = common::hmac(CoseAlg::Hmac256_256).verify_key();
    let p256_public = match &p {
        cratestack_cose::CoseVerifyKey::P256(key) => hex(key.to_sec1_point(false).as_bytes()),
        _ => unreachable!(),
    };
    let ed_public = match &ed {
        cratestack_cose::CoseVerifyKey::Ed25519(key) => hex(key.as_bytes()),
        _ => unreachable!(),
    };
    serde_json::json!({
        "_comment": "TEST KEYS. Published in this repository; never use them outside tests.",
        "ed25519": { "seed": hex(&common::ED25519_SEED), "public": ed_public,
            "thumbprint": hex(&ed.thumbprint()), "kid": hex(&ed.kid()) },
        "p256": { "scalar": common::P256_SCALAR, "public_sec1_uncompressed": p256_public,
            "thumbprint": hex(&p.thumbprint()), "kid": hex(&p.kid()) },
        "hmac": { "secret": hex(&common::HMAC_SECRET),
            "thumbprint": hex(&mac.thumbprint()), "kid": hex(&mac.kid()) },
    })
}

fn fixture_json() -> serde_json::Value {
    let payment = common::fixture::payment();
    serde_json::json!({
        "_comment": "The 7-field payment row, reconstructed; see tests/common/fixture.rs.",
        "codec": "CborCodec (minicbor-serde, string keys)",
        "fields": {
            "id": payment.id.to_string(), "payer": payment.payer, "amount": payment.amount,
            "currency": payment.currency, "status": payment.status,
            "created_at": payment.created_at, "note": payment.note,
        },
        "cbor": hex(&payment_bytes()),
        "size": payment_bytes().len(),
    })
}

fn check_or_write(name: &str, value: &serde_json::Value) {
    let path = vectors_dir().join(name);
    let rendered = serde_json::to_string_pretty(value).expect("json") + "\n";
    if std::env::var("CRATESTACK_COSE_WRITE_VECTORS").as_deref() == Ok("1") {
        std::fs::write(&path, &rendered).expect("write vectors");
    }
    let on_disk = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} missing: {error}", path.display()));
    let on_disk: serde_json::Value = serde_json::from_str(&on_disk).expect("vector json");
    assert_eq!(&on_disk, value, "{name} does not match the derived vectors");
}

#[tokio::test]
async fn checked_in_vectors_match() {
    let cases = derive_cases().await;
    let unary = serde_json::json!({
        "_comment": "Unary COSE vectors (ADR 0006 §§3-5). Hex throughout. Payload: payment-fixture.json; keys: keys.json.",
        "payload": hex(&payment_bytes()),
        "cases": cases,
    });
    check_or_write("unary.json", &unary);
    check_or_write("keys.json", &keys_json());
    check_or_write("payment-fixture.json", &fixture_json());
}

#[test]
fn payment_fixture_is_112_bytes() {
    assert_eq!(payment_bytes().len(), 112);
}
