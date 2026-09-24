//! The shared, language-neutral vectors (ADR 0006 P0, §11), and the sizes
//! ADR 0006 §3 measured.
//!
//! `tests/vectors/*.json` hold hex only, so the wasm, napi, TypeScript and
//! Dart bindings can check themselves against the same bytes. This test
//! re-derives every vector from the fixed inputs and compares; it never
//! rewrites the files unless `CRATESTACK_COSE_WRITE_VECTORS=1` is set.
//! Regenerating is not how a mismatch gets fixed: the bytes are also
//! checked against `coset` (`interop_coset.rs`), the to-be-signed hex here
//! is computed by `coset`, not by the crate, and the sizes are checked
//! against the hand-computed breakdown in `sizes.rs`, all independent of
//! the files.
//!
//! Positive cases must open; `negative` cases must be refused with the
//! coarse `401`, by a verifier holding exactly the keys each one names.

mod common;

use std::borrow::Cow;
use std::path::PathBuf;

use bytes::Bytes;
use common::fixture::payment_bytes;
use common::{CTI_2, CTI_16, IAT, hex, rest_request, rpc_request, unhex};
use cratestack_codec_cbor::CborCodec;
use cratestack_core::rpc::RpcErrorBody;
use cratestack_core::{Binding, CratestackCodec};
use cratestack_cose::{
    CoseAlg, CoseMode, CoseVerifyKey, Ed25519Signer, RequestNonce, external_aad, request_digest,
    request_digest_unsigned,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Case {
    name: String,
    alg_id: i64,
    direction: String,
    /// `false` for ESP256: another implementation's signature need not be
    /// byte-identical (ECDSA may be randomized); verify it instead. These
    /// bytes come from RFC 6979, normalised to low-`s`.
    deterministic: bool,
    binding: BindingJson,
    /// Responses to a signed request: the case whose `cose` bytes the
    /// `request_digest` hashes.
    request_digest_of: Option<String>,
    /// Responses to an unsigned request: `request_digest =
    /// SHA-256(request_nonce ‖ request_payload)`.
    request_nonce: Option<String>,
    request_payload: Option<String>,
    iat: Option<u64>,
    cti: Option<String>,
    payload: String,
    external_aad: String,
    protected: String,
    to_be_signed: String,
    cose: String,
    size: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct BindingJson {
    audience: String,
    method: String,
    route: String,
    path_params: Vec<String>,
    query: Option<String>,
    schema_sha: String,
    payload_type: String,
    request_digest: Option<String>,
    status: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Negative {
    name: String,
    direction: String,
    /// The binding the verifier rebuilds.
    binding: BindingJson,
    /// The keys the verifier's resolver holds (names from `keys.json`).
    verifier_keys: Vec<String>,
    /// The verifier's clock, Unix seconds.
    verifier_now: u64,
    cose: String,
    expected: String,
    why: String,
}

/// The fixed `Cratestack-Nonce` of the unsigned-request vectors.
const REQUEST_NONCE: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90";

fn binding_json(bind: &Binding<'_>) -> BindingJson {
    BindingJson {
        audience: bind.audience.to_string(),
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

fn nonce() -> RequestNonce {
    RequestNonce::from_bytes(unhex(REQUEST_NONCE).try_into().expect("16 bytes"))
}

/// A bodiless REST `GET` of one payment.
fn rest_get() -> Binding<'static> {
    Binding {
        method: Cow::Borrowed("GET"),
        query: None,
        ..rest_request()
    }
}

fn error_body() -> Vec<u8> {
    let body = RpcErrorBody {
        code: "not_found".to_owned(),
        message: "payment not found".to_owned(),
        details: None,
    };
    CborCodec.encode(&body).expect("encode error body")
}

/// Fill in everything a case derives from its sealed bytes.
#[allow(clippy::too_many_arguments)]
fn case(
    name: String,
    alg: CoseAlg,
    bind: &Binding<'_>,
    payload: &[u8],
    sealed: &[u8],
    claims: Option<(u64, &str)>,
    link: (Option<String>, Option<(RequestNonce, &[u8])>),
) -> Case {
    let parts = common::forge::layout(sealed);
    let protected = &sealed[parts.protected.clone()];
    let aad = external_aad(bind).expect("aad");
    let tbs = match alg.mode() {
        CoseMode::Sign1 => common::forge::sign1_tbs(protected, &aad, payload),
        CoseMode::Mac0 => common::forge::mac0_tbs(protected, &aad, payload),
    };
    Case {
        name,
        alg_id: alg.id(),
        direction: if claims.is_some() {
            "request"
        } else {
            "response"
        }
        .to_owned(),
        deterministic: alg != CoseAlg::Esp256,
        binding: binding_json(bind),
        request_digest_of: link.0,
        request_nonce: link.1.map(|(nonce, _)| hex(nonce.as_bytes())),
        request_payload: link.1.map(|(_, payload)| hex(payload)),
        iat: claims.map(|(iat, _)| iat),
        cti: claims.map(|(_, cti)| cti.to_owned()),
        payload: hex(payload),
        external_aad: hex(&aad),
        protected: hex(protected),
        to_be_signed: hex(&tbs),
        cose: hex(sealed),
        size: sealed.len(),
    }
}

async fn open_response(alg: CoseAlg, sealed: &Bytes, bind: &Binding<'_>, payload: &[u8]) {
    let opened = common::client(alg, IAT, CTI_16)
        .open_response(sealed.clone(), bind)
        .await
        .expect("a response vector must open");
    assert_eq!(opened.payload.as_ref(), payload);
}

/// Derive every positive case from the fixed inputs, and check each opens.
async fn derive_cases() -> Vec<Case> {
    let payload = payment_bytes();
    let mut cases = Vec::new();
    for (binding_name, request) in [("rpc", rpc_request()), ("rest", rest_request())] {
        for &alg in CoseAlg::ALL {
            let mut first = None;
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
                let name = format!("{binding_name}-request-{}-{cti_name}", alg_name(alg));
                cases.push(case(
                    name.clone(),
                    alg,
                    &request,
                    &payload,
                    &sealed,
                    Some((IAT, cti)),
                    (None, None),
                ));
                first.get_or_insert((name, sealed));
            }
            let (request_name, request_body) = first.expect("sealed a request");
            // A 200 with the payment, and (RPC) a signed 404 error body.
            let mut responses = vec![("response", 200, payload.clone())];
            if binding_name == "rpc" {
                responses.push(("error-response", 404, error_body()));
            }
            for (kind, status, body) in responses {
                let response = common::response_to(&request, &request_body, status);
                let sealed = common::server(alg, IAT)
                    .seal_response(&body, &response)
                    .await
                    .expect("seal response");
                open_response(alg, &sealed, &response, &body).await;
                let name = format!("{binding_name}-{kind}-{}", alg_name(alg));
                let link = (Some(request_name.clone()), None);
                cases.push(case(name, alg, &response, &body, &sealed, None, link));
            }
        }
    }
    // A signed response to an UNSIGNED, bodiless GET: bound through the
    // client's Cratestack-Nonce.
    for &alg in CoseAlg::ALL {
        let response = Binding {
            request_digest: Some(request_digest_unsigned(&nonce(), b"")),
            status: Some(200),
            ..rest_get()
        };
        let sealed = common::server(alg, IAT)
            .seal_response(&payload, &response)
            .await
            .expect("seal response");
        open_response(alg, &sealed, &response, &payload).await;
        let name = format!("rest-get-response-{}-unsigned-request", alg_name(alg));
        let link = (None, Some((nonce(), &b""[..])));
        cases.push(case(name, alg, &response, &payload, &sealed, None, link));
    }
    // An empty query binds as `null`: these bytes equal the plain RPC case.
    let empty_query = Binding {
        query: Some(Cow::Borrowed("")),
        ..rpc_request()
    };
    let sealed = common::sealed_request(CoseAlg::Ed25519, &empty_query).await;
    assert_eq!(
        sealed,
        common::sealed_request(CoseAlg::Ed25519, &rpc_request()).await
    );
    let name = "rpc-request-sign1-ed25519-cti16-empty-query".to_owned();
    cases.push(case(
        name,
        CoseAlg::Ed25519,
        &empty_query,
        &payload,
        &sealed,
        Some((IAT, CTI_16)),
        (None, None),
    ));
    cases
}

/// Every response's `request_digest` really is the digest of what it
/// links to.
fn check_links(cases: &[Case]) {
    for case in cases.iter().filter(|case| case.direction == "response") {
        let expected = match (
            &case.request_digest_of,
            &case.request_nonce,
            &case.request_payload,
        ) {
            (Some(of), None, None) => {
                let request = cases
                    .iter()
                    .find(|other| &other.name == of)
                    .expect("linked case");
                request_digest(&unhex(&request.cose))
            }
            (None, Some(nonce), Some(payload)) => request_digest_unsigned(
                &RequestNonce::from_bytes(unhex(nonce).try_into().expect("16")),
                &unhex(payload),
            ),
            other => panic!("{}: bad link {other:?}", case.name),
        };
        assert_eq!(
            case.binding.request_digest.as_deref(),
            Some(hex(&expected).as_str()),
            "{}",
            case.name
        );
    }
}

#[path = "common/negative_vectors.rs"]
mod negative;

fn vectors_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/vectors")
}

fn keys_json() -> serde_json::Value {
    let key = |key: &CoseVerifyKey| serde_json::json!({ "thumbprint": hex(&key.thumbprint()), "kid": hex(&key.kid()) });
    let ed = common::ed25519().verify_key();
    let other = Ed25519Signer::from_seed(&common::OTHER_ED25519_SEED).verify_key();
    let p = common::p256().verify_key();
    let mac = common::hmac(CoseAlg::Hmac256_256).verify_key();
    let mut out = serde_json::json!({
        "_comment": "TEST KEYS. Published in this repository; never use them outside tests. The HMAC secret is one key per algorithm (hmac-256-64, hmac-256-256): an HMAC key verifies only the algorithm it is configured for.",
        "ed25519": { "seed": hex(&common::ED25519_SEED), "public": hex(&ed.ed25519_bytes().expect("ed")) },
        "ed25519_other": { "seed": hex(&common::OTHER_ED25519_SEED), "public": hex(&other.ed25519_bytes().expect("ed")) },
        "p256": { "scalar": common::P256_SCALAR, "public_sec1_uncompressed": hex(&p.p256_sec1_uncompressed().expect("p256")) },
        "hmac": { "secret": hex(&common::HMAC_SECRET) },
    });
    for (name, k) in [
        ("ed25519", &ed),
        ("ed25519_other", &other),
        ("p256", &p),
        ("hmac", &mac),
    ] {
        let fields = out[name].as_object_mut().expect("object");
        for (field, value) in key(k).as_object().expect("object") {
            fields.insert(field.clone(), value.clone());
        }
    }
    out
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
    check_links(&cases);
    let negatives = negative::derive().await;
    for vector in &negatives {
        negative::check_rejected(vector).await;
    }
    let unary = serde_json::json!({
        "_comment": "Unary COSE vectors (ADR 0006 §§3-5, and the 2026-09-24 decisions on cratestack#1005: audience in the AAD, Cratestack-Nonce for unsigned requests). Hex throughout. Payload: payment-fixture.json; keys: keys.json. `to_be_signed` is the logical Sig_structure / MAC_structure, even where the implementation hashes it incrementally. Every `negative` vector must be refused (401 unauthenticated) by a verifier holding exactly `verifier_keys`, at `verifier_now`.",
        "payload": hex(&payment_bytes()),
        "cases": cases,
        "negative": negatives,
    });
    check_or_write("unary.json", &unary);
    check_or_write("keys.json", &keys_json());
    check_or_write("payment-fixture.json", &fixture_json());
}

#[test]
fn payment_fixture_is_112_bytes() {
    assert_eq!(payment_bytes().len(), 112);
}
