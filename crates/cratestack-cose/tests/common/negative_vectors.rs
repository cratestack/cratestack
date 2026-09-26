//! The must-reject half of the shared vectors: messages a conforming
//! verifier refuses, each built so that exactly one rule is what refuses
//! it, plus the verifier every vector (positive or negative) is checked
//! with. Included by `tests/vectors.rs` only.

use std::borrow::Cow;
use std::sync::Arc;

use bytes::Bytes;
use cratestack_core::{
    Binding, BoundHeaders, CratestackError, InMemoryNonceStore, PathParams, RequestDigest,
    ResponseBinding,
};
use cratestack_cose::{
    CoseAlg, CoseEnvelope, CoseMode, CoseVerifyKey, DEFAULT_SKEW_SECS, Ed25519Signer, Opened,
    RequestNonce, StaticVerifierResolver, UNAUTHENTICATED, external_aad, request_digest,
    request_digest_unsigned,
};

use super::{BindingJson, Negative, binding_json, nonce, rest_get};
use crate::common::forge::{self, TAG_MAC0};
use crate::common::{self, CTI_2, CTI_16, IAT, hex, rest_request, rpc_request, unhex};

/// What a negative vector expects: the coarse `401` for every check on the
/// received bytes, a `500` for local misuse (refused before the body is
/// read, so it is not an oracle).
const REJECT_401: &str = "reject: 401 unauthenticated";
const REJECT_500: &str = "reject: 500 local misuse, before the body is read";

/// The verification key a `keys.json` entry names.
pub(super) fn key(name: &str) -> CoseVerifyKey {
    match name {
        "ed25519" => common::ed25519().verify_key(),
        "ed25519_other" => Ed25519Signer::from_seed(&common::OTHER_ED25519_SEED).verify_key(),
        "p256" => common::p256().verify_key(),
        "hmac-256-64" => common::hmac(CoseAlg::Hmac256_64).verify_key(),
        "hmac-256-256" => common::hmac(CoseAlg::Hmac256_256).verify_key(),
        other => panic!("unknown key {other}"),
    }
}

/// A vector's `mode` field.
pub(super) fn mode_name(mode: CoseMode) -> &'static str {
    match mode {
        CoseMode::Sign1 => "sign1",
        CoseMode::Mac0 => "mac0",
    }
}

/// A negative vector, opened in `mode` at `IAT` with the default skew.
fn negative(
    name: &str,
    direction: &str,
    mode: CoseMode,
    bind: &Binding<'_>,
    keys: &[&str],
    cose: &[u8],
    why: &str,
) -> Negative {
    Negative {
        name: format!("neg-{name}"),
        direction: direction.to_owned(),
        mode: mode_name(mode).to_owned(),
        binding: binding_json(bind),
        verifier_keys: keys.iter().map(|key| (*key).to_owned()).collect(),
        verifier_now: IAT,
        skew_secs: DEFAULT_SKEW_SECS,
        cose: hex(cose),
        expected: REJECT_401.to_owned(),
        why: why.to_owned(),
    }
}

fn high_s_twin(body: &[u8]) -> Vec<u8> {
    let range = forge::layout(body).signature;
    let sig = p256::ecdsa::Signature::from_slice(&body[range.clone()]).expect("sig");
    let (r, s) = sig.split_scalars();
    let high: p256::Scalar = -*s;
    let twin = p256::ecdsa::Signature::from_scalars(r.to_bytes(), high.to_bytes()).expect("twin");
    let mut out = body.to_vec();
    out[range].copy_from_slice(&twin.to_bytes());
    out
}

fn ed25519_with_kid(kid: &[u8], aad: &[u8]) -> Vec<u8> {
    let protected =
        forge::request_protected(-19, kid, u32::try_from(IAT).expect("u32"), &unhex(CTI_16));
    forge::ed25519_request(&protected, aad, &common::fixture::payment_bytes())
}

/// `body` with the byte at `at` XORed with 1.
fn flipped(body: &[u8], at: usize) -> Vec<u8> {
    let mut out = body.to_vec();
    out[at] ^= 0x01;
    out
}

/// `body` with its payload `bstr` head (`0x58 len`, as sealed) rewritten
/// as the non-minimal `0x59 0x00 len`. The signature covers the canonical
/// `MAC_structure` / `Sig_structure`, so a lenient parser would accept it.
fn non_minimal_payload_head(body: &[u8]) -> Vec<u8> {
    let payload = forge::layout(body).payload;
    let head = payload.start - 2;
    assert_eq!(body[head], 0x58, "a 2-byte payload head");
    let mut out = body[..head].to_vec();
    out.extend_from_slice(&[0x59, 0x00, body[head + 1]]);
    out.extend_from_slice(&body[payload.start..]);
    out
}

pub(super) async fn derive() -> Vec<Negative> {
    let rest = rest_request();
    let rpc = rpc_request();
    let es256 = common::sealed_request(CoseAlg::Esp256, &rest).await;
    let mac64 = common::sealed_request(CoseAlg::Hmac256_64, &rpc).await;
    let ed = common::sealed_request(CoseAlg::Ed25519, &rpc).await;
    let ed_layout = forge::layout(&ed);
    let rpc_aad = external_aad(&rpc).expect("aad");
    let victim_kid = common::ed25519().verify_key().kid();
    let impostor = CoseEnvelope::client(
        CoseMode::Sign1,
        Arc::new(KidOf(
            Ed25519Signer::from_seed(&common::OTHER_ED25519_SEED),
            victim_kid,
        )),
        common::resolver(),
    )
    .clock(|| i64::try_from(IAT).expect("fits"))
    .cti_source(|| Ok(unhex(CTI_16)))
    .build()
    .expect("client")
    .seal_request(&common::fixture::payment_bytes(), &rpc)
    .await
    .expect("seal");
    let mut unprotected_kid = vec![0xa1, 0x04];
    unprotected_kid.extend(forge::bstr(&victim_kid));
    let deprecated = forge::request_protected(
        -8,
        &victim_kid,
        u32::try_from(IAT).expect("u32"),
        &unhex(CTI_16),
    );
    let deprecated =
        forge::ed25519_request(&deprecated, &rpc_aad, &common::fixture::payment_bytes());
    let get_a = common::answering(&rest_get(), request_digest_unsigned(&nonce(), b""), 200);
    let get_b = common::answering(
        &rest_get(),
        request_digest_unsigned(&RequestNonce::from_bytes([0x5a; 16]), b""),
        200,
    );
    let answer_a = common::server(CoseAlg::Ed25519, IAT)
        .seal_response(&common::fixture::payment_bytes(), &get_a)
        .await
        .expect("seal");
    let mut trailing = ed.to_vec();
    trailing.push(0x00);
    let mut retagged = ed.to_vec();
    retagged[0] = TAG_MAC0;
    let answer_to_ed = common::response_to(&rpc, &ed, 200);
    let answer_ed = common::server(CoseAlg::Ed25519, IAT)
        .seal_response(&common::fixture::payment_bytes(), &answer_to_ed)
        .await
        .expect("seal");
    let other_request = common::client(CoseAlg::Ed25519, IAT, CTI_2)
        .seal_request(&common::fixture::payment_bytes(), &rpc)
        .await
        .expect("seal");
    // The digest-form confusion (2026-09-25): the server answered an
    // UNSIGNED request whose nonce and body are `ed[..16]` and `ed[16..]`,
    // so its digest is SHA-256(ed), the same as the signed request's.
    let twin_nonce = RequestNonce::from_bytes(ed[..16].try_into().expect("16 bytes"));
    let twin_digest = request_digest_unsigned(&twin_nonce, &ed[16..]);
    assert_eq!(twin_digest.digest, request_digest(&ed).digest);
    let answer_twin = common::server(CoseAlg::Ed25519, IAT)
        .seal_response(
            b"\xa1eerrorkbad request",
            &common::answering(&rpc, twin_digest, 400),
        )
        .await
        .expect("seal");
    // A request correctly signed over an AAD whose audience is empty,
    // which `external_aad` refuses to encode: the test encodes it by hand.
    let mut empty_aad = rpc_aad.clone();
    let audience = [&[0x68][..], b"payments"].concat();
    let at = empty_aad
        .windows(audience.len())
        .position(|window| window == audience)
        .expect("audience in the AAD");
    empty_aad.splice(at..at + audience.len(), [0x60]);
    let sign1 = CoseMode::Sign1;
    let mut vectors = vec![
        negative(
            "esp256-high-s",
            "request",
            sign1,
            &rest,
            &["p256"],
            &high_s_twin(&es256),
            "the valid request's ESP256 signature with s replaced by n - s: it verifies mathematically, but only low-s is accepted, so a third party cannot re-spell a signed message",
        ),
        negative(
            "hmac-64-bit-tag-for-a-256-256-key",
            "request",
            CoseMode::Mac0,
            &rpc,
            &["hmac-256-256"],
            &mac64,
            "a valid HMAC 256/64 (alg 4) request; the verifier's key for this secret is configured for HMAC 256/256 only",
        ),
        negative(
            "kid-7-bytes",
            "request",
            sign1,
            &rpc,
            &["ed25519"],
            &ed25519_with_kid(&[1; 7], &rpc_aad),
            "correctly signed, but the kid is 7 bytes, not 8",
        ),
        negative(
            "kid-9-bytes",
            "request",
            sign1,
            &rpc,
            &["ed25519"],
            &ed25519_with_kid(&[1; 9], &rpc_aad),
            "correctly signed, but the kid is 9 bytes, not 8",
        ),
        negative(
            "kid-of-another-key",
            "request",
            sign1,
            &rpc,
            &["ed25519", "ed25519_other"],
            &impostor,
            "signed by ed25519_other under ed25519's kid; the verifier holds both keys, and only a key whose own kid is the header's may verify. A resolver that returns keys by kid already filters ed25519_other out, so there this vector exercises only the signature check; it exercises the opener's own kid-equality check in an implementation whose resolver does not pre-filter by kid",
        ),
        negative(
            "wrong-audience",
            "request",
            sign1,
            &Binding {
                audience: Cow::Borrowed("ledger"),
                ..rpc.clone()
            },
            &["ed25519"],
            &ed,
            "a valid request sealed for audience \"payments\", opened by the service \"ledger\"",
        ),
        negative(
            "kid-in-unprotected-header",
            "request",
            sign1,
            &rpc,
            &["ed25519"],
            &forge::with_unprotected(&ed, &unprotected_kid),
            "a valid request with {4: kid} added to the unprotected header, which must be empty",
        ),
        negative(
            "alg-minus-8",
            "request",
            sign1,
            &rpc,
            &["ed25519"],
            &deprecated,
            "correctly signed Ed25519 under the deprecated polymorphic alg -8 (RFC 9864)",
        ),
        negative(
            "trailing-byte",
            "request",
            sign1,
            &rpc,
            &["ed25519"],
            &trailing,
            "a valid request followed by one extra byte",
        ),
        negative(
            "response-to-another-get",
            "response",
            sign1,
            &get_b,
            &["ed25519"],
            &answer_a,
            "a signed response to an unsigned GET carrying one Cratestack-Nonce, presented as the answer to a GET of the same URL with another nonce",
        ),
        negative(
            "tampered-payload",
            "request",
            sign1,
            &rpc,
            &["ed25519"],
            &flipped(&ed, ed_layout.payload.end - 1),
            "a valid request with the last payload byte's low bit flipped",
        ),
        negative(
            "tampered-protected-header",
            "request",
            sign1,
            &rpc,
            &["ed25519"],
            &flipped(&ed, ed_layout.protected.start + 20),
            "a valid request with iat's last byte's low bit flipped in the protected header: still well-formed and fresh, but not what was signed",
        ),
        negative(
            "aad-route-mismatch",
            "request",
            sign1,
            &Binding {
                route: Cow::Borrowed("model.Payment.refund"),
                ..rpc.clone()
            },
            &["ed25519"],
            &ed,
            "a valid request for model.Payment.create, opened as model.Payment.refund",
        ),
        negative(
            "schema-sha-mismatch",
            "request",
            sign1,
            &Binding {
                schema_sha: [0; 32],
                ..rpc.clone()
            },
            &["ed25519"],
            &ed,
            "a valid request, opened by a verifier built against another schema (schema_sha all zero)",
        ),
        negative(
            "request-digest-mismatch",
            "response",
            sign1,
            &common::response_to(&rpc, &other_request, 200),
            &["ed25519"],
            &answer_ed,
            "a signed response to rpc-request-sign1-ed25519-cti16, presented as the answer to rpc-request-sign1-ed25519-cti2",
        ),
        negative(
            "stale-iat",
            "request",
            sign1,
            &rpc,
            &["ed25519"],
            &ed,
            "a valid request with iat = 1790000000, opened when the verifier's clock reads iat + skew + 1",
        ),
        negative(
            "wrong-tag",
            "request",
            sign1,
            &rpc,
            &["ed25519"],
            &retagged,
            "a valid COSE_Sign1 request re-tagged 17 (COSE_Mac0), opened by a Sign1 verifier",
        ),
        negative(
            "non-minimal-cbor-head",
            "request",
            sign1,
            &rpc,
            &["ed25519"],
            &non_minimal_payload_head(&ed),
            "a valid request whose payload bstr head is re-encoded as 0x59 0x00 0x70 instead of 0x58 0x70; the signature still verifies over the canonical Sig_structure, so only strict parsing refuses it",
        ),
        negative(
            "request-kind-mismatch",
            "response",
            sign1,
            &common::response_to(&rpc, &ed, 400),
            &["ed25519"],
            &answer_twin,
            "the server's answer to an UNSIGNED request whose Cratestack-Nonce is the first 16 bytes of rpc-request-sign1-ed25519-cti16 and whose body is the rest: its digest equals the signed request's (and the status, 400, is the one the client expects), so only request_kind (0, not 1) tells them apart",
        ),
        negative(
            "empty-audience",
            "request",
            sign1,
            &Binding {
                audience: Cow::Borrowed(""),
                ..rpc.clone()
            },
            &["ed25519"],
            &forge::ed25519_request(
                &ed[ed_layout.protected.clone()],
                &empty_aad,
                &common::fixture::payment_bytes(),
            ),
            "correctly signed over an AAD whose audience is the empty string; an empty audience binds no recipient, so the verifier refuses it as misuse before reading the body",
        ),
    ];
    for vector in &mut vectors {
        match vector.name.as_str() {
            "neg-stale-iat" => vector.verifier_now = IAT + DEFAULT_SKEW_SECS + 1,
            "neg-empty-audience" => vector.expected = REJECT_500.to_owned(),
            _ => {}
        }
    }
    vectors
}

/// A signer that claims another key's `kid`.
struct KidOf(Ed25519Signer, [u8; 8]);

#[async_trait::async_trait]
impl cratestack_cose::CoseSigner for KidOf {
    fn alg(&self) -> CoseAlg {
        CoseAlg::Ed25519
    }
    fn kid(&self) -> &[u8] {
        &self.1
    }
    async fn sign(&self, tbs: &[u8]) -> Result<Vec<u8>, CratestackError> {
        self.0.sign(tbs).await
    }
}

/// A `Binding` rebuilt from a vector's JSON alone.
pub(super) fn binding_from_json(json: &BindingJson) -> Binding<'static> {
    let digest = |hex: &String| -> [u8; 32] { unhex(hex).try_into().expect("32") };
    let response = match (&json.request_kind, &json.request_digest, json.status) {
        (Some(kind), Some(request_digest), Some(status)) => Some(ResponseBinding {
            request: RequestDigest {
                kind: common::request_kind(u64::from(*kind)),
                digest: digest(request_digest),
            },
            status,
        }),
        (None, None, None) => None,
        other => panic!("a half response binding in a vector: {other:?}"),
    };
    Binding {
        audience: Cow::Owned(json.audience.clone()),
        method: Cow::Owned(json.method.clone()),
        route: Cow::Owned(json.route.clone()),
        path_params: PathParams::Owned(json.path_params.clone()),
        query: json.query.clone().map(Cow::Owned),
        schema_sha: digest(&json.schema_sha),
        payload_media_type: Cow::Owned(json.payload_type.clone()),
        bound_headers: BoundHeaders {
            idempotency_key: json.bound_headers.idempotency_key.clone().map(Cow::Owned),
            if_match: json.bound_headers.if_match.clone().map(Cow::Owned),
        },
        response,
    }
}

/// Open `cose` the way another implementation would, from a vector's
/// fields alone: an envelope in `mode` whose resolver holds exactly `keys`,
/// whose clock reads `now`, with `skew` seconds of skew.
pub(super) async fn open_as_vector(
    direction: &str,
    mode: &str,
    keys: &[String],
    now: u64,
    skew: u64,
    bind: &BindingJson,
    cose: &str,
) -> Result<Opened, CratestackError> {
    let mut resolver = StaticVerifierResolver::new();
    for name in keys {
        resolver = resolver.with_key(key(name));
    }
    let resolver = Arc::new(resolver);
    // The envelope needs a signer of its mode to build; it never signs here.
    let alg = match mode {
        "sign1" => CoseAlg::Ed25519,
        "mac0" => CoseAlg::Hmac256_256,
        other => panic!("unknown mode {other}"),
    };
    let now = i64::try_from(now).expect("fits");
    let skew = std::time::Duration::from_secs(skew);
    let bind = binding_from_json(bind);
    let body = Bytes::from(unhex(cose));
    if direction == "request" {
        let store = Arc::new(InMemoryNonceStore::new());
        CoseEnvelope::server(alg.mode(), common::signer(alg), resolver, store)
            .clock(move || now)
            .skew(skew)
            .build()
            .expect("server")
            .open_request(body, &bind)
            .await
    } else {
        CoseEnvelope::client(alg.mode(), common::signer(alg), resolver)
            .clock(move || now)
            .skew(skew)
            .build()
            .expect("client")
            .open_response(body, &bind)
            .await
    }
}

/// Require the refusal `vector.expected` names.
pub(super) async fn check_rejected(vector: &Negative) {
    let result = open_as_vector(
        &vector.direction,
        &vector.mode,
        &vector.verifier_keys,
        vector.verifier_now,
        vector.skew_secs,
        &vector.binding,
        &vector.cose,
    )
    .await;
    match (vector.expected.as_str(), result) {
        (REJECT_401, Err(CratestackError::Unauthorized(message))) => {
            assert_eq!(message, UNAUTHENTICATED, "{}", vector.name)
        }
        (REJECT_500, Err(CratestackError::Internal(_))) => {}
        (_, other) => panic!(
            "{} must be refused ({}), got {other:?}",
            vector.name, vector.expected
        ),
    }
}
