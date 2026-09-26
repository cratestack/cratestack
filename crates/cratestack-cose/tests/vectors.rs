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
//! The files are self-contained for another implementation (the P1
//! ports): every key carries its algorithm, every positive case names the
//! key that verifies it (`key`) and, for a request, the verifier's clock
//! and skew; every negative case names its verifier's mode, keys, clock and
//! skew. Positive cases must open with exactly that one key; `negative`
//! cases must be refused as `expected` says (the coarse `401`, or a `500`
//! for local misuse), by a verifier holding exactly the keys each names.
//! Both are checked here the way a port would check them, from the JSON.
//!
//! **An ESP256 sender MUST emit low-`s`**: verifiers refuse a high `s`
//! (`neg-esp256-high-s`), so a port whose signer may return either
//! normalises before sending.

mod common;

use std::borrow::Cow;
use std::path::PathBuf;

use bytes::Bytes;
use common::fixture::payment_bytes;
use common::{CTI_2, CTI_16, IAT, hex, rest_request, rpc_request, unhex};
use cratestack_codec_cbor::CborCodec;
use cratestack_core::rpc::RpcErrorBody;
use cratestack_core::{Binding, BoundHeaders, CratestackCodec};
use cratestack_cose::{
    CoseAlg, CoseMode, CoseVerifyKey, DEFAULT_SKEW_SECS, Ed25519Signer, RequestNonce, external_aad,
    request_digest, request_digest_unsigned,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Case {
    name: String,
    alg_id: i64,
    direction: String,
    /// The `keys.json` entry that verifies this case, and the only key a
    /// verifier needs to hold for it.
    key: String,
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
    /// Requests: the verifier's clock (Unix seconds) and skew the case
    /// opens at. `None` for a response, which carries no `iat`.
    verifier_now: Option<u64>,
    skew_secs: Option<u64>,
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
    /// `Idempotency-Key` and `If-Match` exactly as sent, `null` when
    /// absent: the AAD's `bound_headers` array, in that order.
    bound_headers: BoundHeadersJson,
    /// Responses: 0 = unsigned request, 1 = signed request.
    request_kind: Option<u8>,
    request_digest: Option<String>,
    status: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct BoundHeadersJson {
    idempotency_key: Option<String>,
    if_match: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Negative {
    name: String,
    direction: String,
    /// The verifier's configured mode (`sign1` / `mac0`), as a router's
    /// comes from the `Content-Type`, not from the message's tag.
    mode: String,
    /// The binding the verifier rebuilds.
    binding: BindingJson,
    /// The keys the verifier's resolver holds (names from `keys.json`).
    verifier_keys: Vec<String>,
    /// The verifier's clock, Unix seconds, and its skew.
    verifier_now: u64,
    skew_secs: u64,
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
        bound_headers: BoundHeadersJson {
            idempotency_key: bind
                .bound_headers
                .idempotency_key
                .as_deref()
                .map(str::to_owned),
            if_match: bind.bound_headers.if_match.as_deref().map(str::to_owned),
        },
        request_kind: bind.response.map(|response| response.request.kind.code()),
        request_digest: bind.response.map(|response| hex(&response.request.digest)),
        status: bind.response.map(|response| response.status),
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

/// The `keys.json` entry for `alg`'s test key.
fn key_name(alg: CoseAlg) -> &'static str {
    match alg {
        CoseAlg::Ed25519 => "ed25519",
        CoseAlg::Esp256 => "p256",
        CoseAlg::Hmac256_64 => "hmac-256-64",
        CoseAlg::Hmac256_256 => "hmac-256-256",
        _ => unreachable!(),
    }
}

fn nonce() -> RequestNonce {
    RequestNonce::from_bytes(unhex(REQUEST_NONCE).try_into().expect("16 bytes"))
}

/// A bodiless REST `GET` of one payment: no query, and of the two bound
/// headers only `Idempotency-Key`, so the vectors also cover a `null` next
/// to a string.
fn rest_get() -> Binding<'static> {
    Binding {
        method: Cow::Borrowed("GET"),
        query: None,
        bound_headers: BoundHeaders {
            idempotency_key: Some(Cow::Borrowed("idem-7f3a")),
            if_match: None,
        },
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
    if alg == CoseAlg::Ed25519 {
        // The sealer streamed both PureEdDSA passes over the structure's
        // pieces; the result must be `SigningKey::sign` over the contiguous
        // `Sig_structure` (built by `coset`), byte for byte.
        assert_eq!(
            &sealed[parts.signature.clone()],
            common::forge::ed25519_sign(&tbs).as_slice(),
            "{name}: streamed Ed25519 differs from contiguous signing"
        );
    }
    Case {
        name,
        alg_id: alg.id(),
        direction: if claims.is_some() {
            "request"
        } else {
            "response"
        }
        .to_owned(),
        key: key_name(alg).to_owned(),
        deterministic: alg != CoseAlg::Esp256,
        binding: binding_json(bind),
        request_digest_of: link.0,
        request_nonce: link.1.map(|(nonce, _)| hex(nonce.as_bytes())),
        request_payload: link.1.map(|(_, payload)| hex(payload)),
        iat: claims.map(|(iat, _)| iat),
        cti: claims.map(|(_, cti)| cti.to_owned()),
        verifier_now: claims.map(|_| IAT),
        skew_secs: claims.map(|_| DEFAULT_SKEW_SECS),
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
        let response = common::answering(&rest_get(), request_digest_unsigned(&nonce(), b""), 200);
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
            Some(hex(&expected.digest).as_str()),
            "{}",
            case.name
        );
        assert_eq!(
            case.binding.request_kind,
            Some(expected.kind.code()),
            "{}: a digest linked to a {} request",
            case.name,
            if case.request_digest_of.is_some() {
                "signed"
            } else {
                "nonce-bound unsigned"
            }
        );
    }
}

/// Open every positive case the way a port would: from its JSON alone,
/// with a verifier holding only the key the case names, at the clock and
/// skew it names.
async fn check_opens_as_vector(cases: &[Case]) {
    for case in cases {
        let mode = negative::mode_name(CoseAlg::from_id(case.alg_id).expect("alg").mode());
        let now = case.verifier_now.unwrap_or(IAT);
        let skew = case.skew_secs.unwrap_or(DEFAULT_SKEW_SECS);
        let opened = negative::open_as_vector(
            &case.direction,
            mode,
            std::slice::from_ref(&case.key),
            now,
            skew,
            &case.binding,
            &case.cose,
        )
        .await
        .unwrap_or_else(|error| panic!("{} must open: {error:?}", case.name));
        assert_eq!(hex(&opened.payload), case.payload, "{}", case.name);
    }
}

#[path = "common/negative_vectors.rs"]
mod negative;

fn vectors_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/vectors")
}

fn keys_json() -> serde_json::Value {
    let ids = |key: &CoseVerifyKey| {
        serde_json::json!({
            "alg": key.alg().id(),
            "thumbprint": hex(&key.thumbprint()),
            "kid": hex(&key.kid()),
        })
    };
    let ed = common::ed25519().verify_key();
    let other = Ed25519Signer::from_seed(&common::OTHER_ED25519_SEED).verify_key();
    let p = common::p256().verify_key();
    let mac64 = common::hmac(CoseAlg::Hmac256_64).verify_key();
    let mac256 = common::hmac(CoseAlg::Hmac256_256).verify_key();
    let secret = hex(&common::HMAC_SECRET);
    let mut out = serde_json::json!({
        "_comment": "TEST KEYS. Published in this repository; never use them outside tests. Every entry is one verification key bound to exactly one algorithm (`alg`, the IANA COSE value); a vector's `key` / `verifier_keys` name these entries. hmac-256-64 and hmac-256-256 are the same secret configured for two algorithms, so they share a kid; each verifies only its own algorithm.",
        "ed25519": { "seed": hex(&common::ED25519_SEED), "public": hex(&ed.ed25519_bytes().expect("ed")) },
        "ed25519_other": { "seed": hex(&common::OTHER_ED25519_SEED), "public": hex(&other.ed25519_bytes().expect("ed")) },
        "p256": { "scalar": common::P256_SCALAR, "public_sec1_uncompressed": hex(&p.p256_sec1_uncompressed().expect("p256")) },
        "hmac-256-64": { "secret": secret },
        "hmac-256-256": { "secret": secret },
    });
    for (name, k) in [
        ("ed25519", &ed),
        ("ed25519_other", &other),
        ("p256", &p),
        ("hmac-256-64", &mac64),
        ("hmac-256-256", &mac256),
    ] {
        // The file's names are the ones the vectors check against.
        assert_eq!(negative::key(name), *k, "{name}");
        let fields = out[name].as_object_mut().expect("object");
        for (field, value) in ids(k).as_object().expect("object") {
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
    check_opens_as_vector(&cases).await;
    let negatives = negative::derive().await;
    for vector in &negatives {
        negative::check_rejected(vector).await;
    }
    let unary = serde_json::json!({
        "_comment": "Unary COSE vectors (ADR 0006 §§3-5, and the decisions on cratestack#1005: audience in the AAD and Cratestack-Nonce for unsigned requests, 2026-09-24; request_kind in the response AAD and a non-empty audience, 2026-09-25; and on cratestack#1006: bound_headers [Idempotency-Key, If-Match] after payload_type, 2026-09-26). Hex throughout. Payload: payment-fixture.json; keys: keys.json. Every case must open with a verifier holding only the key its `key` names (requests: at `verifier_now`, with `skew_secs`). `to_be_signed` is the logical Sig_structure / MAC_structure, even where the implementation computes over it in pieces. ESP256 senders MUST emit low-s: verifiers refuse a high s (neg-esp256-high-s). Every `negative` vector must be refused as `expected` says by a verifier in `mode` holding exactly `verifier_keys`, at `verifier_now` with `skew_secs`.",
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
