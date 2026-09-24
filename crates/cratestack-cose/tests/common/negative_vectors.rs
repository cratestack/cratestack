//! The must-reject half of the shared vectors: messages a conforming
//! verifier refuses, each built so that exactly one rule is what refuses
//! it. Included by `tests/vectors.rs` only.

use std::borrow::Cow;
use std::sync::Arc;

use bytes::Bytes;
use cratestack_core::{Binding, CratestackError, InMemoryNonceStore, PathParams};
use cratestack_cose::{
    CoseAlg, CoseEnvelope, CoseMode, CoseVerifyKey, Ed25519Signer, RequestNonce,
    StaticVerifierResolver, UNAUTHENTICATED, external_aad, request_digest_unsigned,
};

use super::{BindingJson, Negative, binding_json, nonce, rest_get};
use crate::common::forge::{self, TAG_SIGN1};
use crate::common::{self, CTI_16, IAT, hex, rest_request, rpc_request, unhex};

fn key(name: &str) -> CoseVerifyKey {
    match name {
        "ed25519" => common::ed25519().verify_key(),
        "ed25519_other" => Ed25519Signer::from_seed(&common::OTHER_ED25519_SEED).verify_key(),
        "p256" => common::p256().verify_key(),
        "hmac-256-64" => common::hmac(CoseAlg::Hmac256_64).verify_key(),
        "hmac-256-256" => common::hmac(CoseAlg::Hmac256_256).verify_key(),
        other => panic!("unknown key {other}"),
    }
}

fn negative(
    name: &str,
    direction: &str,
    bind: &Binding<'_>,
    keys: &[&str],
    cose: &[u8],
    why: &str,
) -> Negative {
    Negative {
        name: format!("neg-{name}"),
        direction: direction.to_owned(),
        binding: binding_json(bind),
        verifier_keys: keys.iter().map(|key| (*key).to_owned()).collect(),
        verifier_now: IAT,
        cose: hex(cose),
        expected: "reject: 401 unauthenticated".to_owned(),
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

fn ed25519_with_kid(kid: &[u8], bind: &Binding<'_>) -> Vec<u8> {
    let protected =
        forge::request_protected(-19, kid, u32::try_from(IAT).expect("u32"), &unhex(CTI_16));
    forge::ed25519_request(
        &protected,
        &external_aad(bind).expect("aad"),
        &common::fixture::payment_bytes(),
    )
}

pub(super) async fn derive() -> Vec<Negative> {
    let rest = rest_request();
    let rpc = rpc_request();
    let es256 = common::sealed_request(CoseAlg::Esp256, &rest).await;
    let mac64 = common::sealed_request(CoseAlg::Hmac256_64, &rpc).await;
    let ed = common::sealed_request(CoseAlg::Ed25519, &rpc).await;
    let victim_kid = common::ed25519().verify_key().kid();
    let other_key = Ed25519Signer::from_seed(&common::OTHER_ED25519_SEED);
    let impostor = CoseEnvelope::client(
        CoseMode::Sign1,
        Arc::new(KidOf(other_key, victim_kid)),
        common::resolver(),
    )
    .clock(|| i64::try_from(IAT).expect("fits"))
    .cti_source(|| Ok(unhex(CTI_16)))
    .build()
    .expect("client")
    .seal_request(&common::fixture::payment_bytes(), &rpc)
    .await
    .expect("seal");
    let other_audience = Binding {
        audience: Cow::Borrowed("ledger"),
        ..rpc.clone()
    };
    let mut unprotected_kid = vec![0xa1, 0x04];
    unprotected_kid.extend(forge::bstr(&victim_kid));
    let deprecated = forge::request_protected(
        -8,
        &victim_kid,
        u32::try_from(IAT).expect("u32"),
        &unhex(CTI_16),
    );
    let deprecated = forge::ed25519_request(
        &deprecated,
        &external_aad(&rpc).expect("aad"),
        &common::fixture::payment_bytes(),
    );
    let get_a = Binding {
        request_digest: Some(request_digest_unsigned(&nonce(), b"")),
        status: Some(200),
        ..rest_get()
    };
    let get_b = Binding {
        request_digest: Some(request_digest_unsigned(
            &RequestNonce::from_bytes([0x5a; 16]),
            b"",
        )),
        ..get_a.clone()
    };
    let answer_a = common::server(CoseAlg::Ed25519, IAT)
        .seal_response(&common::fixture::payment_bytes(), &get_a)
        .await
        .expect("seal");
    let mut trailing = ed.to_vec();
    trailing.push(0x00);
    vec![
        negative(
            "esp256-high-s",
            "request",
            &rest,
            &["p256"],
            &high_s_twin(&es256),
            "the valid request's ESP256 signature with s replaced by n - s: it verifies mathematically, but only low-s is accepted, so each message has one encoding",
        ),
        negative(
            "hmac-64-bit-tag-for-a-256-256-key",
            "request",
            &rpc,
            &["hmac-256-256"],
            &mac64,
            "a valid HMAC 256/64 (alg 4) request; the verifier's key for this secret is configured for HMAC 256/256 only",
        ),
        negative(
            "kid-7-bytes",
            "request",
            &rpc,
            &["ed25519"],
            &ed25519_with_kid(&[1; 7], &rpc),
            "correctly signed, but the kid is 7 bytes, not 8",
        ),
        negative(
            "kid-9-bytes",
            "request",
            &rpc,
            &["ed25519"],
            &ed25519_with_kid(&[1; 9], &rpc),
            "correctly signed, but the kid is 9 bytes, not 8",
        ),
        negative(
            "kid-of-another-key",
            "request",
            &rpc,
            &["ed25519", "ed25519_other"],
            &impostor,
            "signed by ed25519_other under ed25519's kid; the verifier holds both keys, and only a key whose own kid is the header's may verify",
        ),
        negative(
            "wrong-audience",
            "request",
            &other_audience,
            &["ed25519"],
            &ed,
            "a valid request sealed for audience \"payments\", opened by the service \"ledger\"",
        ),
        negative(
            "kid-in-unprotected-header",
            "request",
            &rpc,
            &["ed25519"],
            &forge::with_unprotected(&ed, &unprotected_kid),
            "a valid request with {4: kid} added to the unprotected header, which must be empty",
        ),
        negative(
            "alg-minus-8",
            "request",
            &rpc,
            &["ed25519"],
            &deprecated,
            "correctly signed Ed25519 under the deprecated polymorphic alg -8 (RFC 9864)",
        ),
        negative(
            "trailing-byte",
            "request",
            &rpc,
            &["ed25519"],
            &trailing,
            "a valid request followed by one extra byte",
        ),
        negative(
            "response-to-another-get",
            "response",
            &get_b,
            &["ed25519"],
            &answer_a,
            "a signed response to an unsigned GET carrying one Cratestack-Nonce, presented as the answer to a GET of the same URL with another nonce",
        ),
    ]
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

fn binding_from_json(json: &BindingJson) -> Binding<'static> {
    let digest = |hex: &String| -> [u8; 32] { unhex(hex).try_into().expect("32") };
    Binding {
        audience: Cow::Owned(json.audience.clone()),
        method: Cow::Owned(json.method.clone()),
        route: Cow::Owned(json.route.clone()),
        path_params: PathParams::Owned(json.path_params.clone()),
        query: json.query.clone().map(Cow::Owned),
        schema_sha: digest(&json.schema_sha),
        payload_media_type: Cow::Owned(json.payload_type.clone()),
        request_digest: json.request_digest.as_ref().map(digest),
        status: json.status,
    }
}

/// Open a negative vector the way another implementation would, from the
/// JSON alone, and require the coarse 401.
pub(super) async fn check_rejected(vector: &Negative) {
    let bind = binding_from_json(&vector.binding);
    let mut resolver = StaticVerifierResolver::new();
    for name in &vector.verifier_keys {
        resolver = resolver.with_key(key(name));
    }
    let resolver = Arc::new(resolver);
    let body = Bytes::from(unhex(&vector.cose));
    let now = i64::try_from(vector.verifier_now).expect("fits");
    // The envelope's mode comes from the message tag, as a router's would
    // come from the Content-Type.
    let alg = if body[0] == TAG_SIGN1 {
        CoseAlg::Ed25519
    } else {
        CoseAlg::Hmac256_256
    };
    let result = if vector.direction == "request" {
        CoseEnvelope::server(
            alg.mode(),
            common::signer(alg),
            resolver,
            Arc::new(InMemoryNonceStore::new()),
        )
        .clock(move || now)
        .build()
        .expect("server")
        .open_request(body, &bind)
        .await
    } else {
        CoseEnvelope::client(alg.mode(), common::signer(alg), resolver)
            .clock(move || now)
            .build()
            .expect("client")
            .open_response(body, &bind)
            .await
    };
    match result {
        Err(CratestackError::Unauthorized(message)) => {
            assert_eq!(message, UNAUTHENTICATED, "{}", vector.name)
        }
        other => panic!("{} must be refused, got {other:?}", vector.name),
    }
}
