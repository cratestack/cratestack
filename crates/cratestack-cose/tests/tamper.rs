//! Tampering (ADR 0006 test plan): every change to the bytes, to the
//! binding, or to the algorithm/tag/key pairing rejects. Assertions are on
//! encoded bytes, never on decoded values.

mod common;

use std::borrow::Cow;
use std::sync::Arc;

use bytes::Bytes;
use common::backends::FixedResolver;
use common::fixture::payment_bytes;
use common::forge::{self, TAG_MAC0, TAG_SIGN1, layout};
use common::{IAT, rest_request};
use cratestack_core::{Binding, CratestackError, InMemoryNonceStore, PathParams, RequestKind};
use cratestack_cose::{CoseAlg, CoseSigner, CoseVerifyKey, UNAUTHENTICATED, external_aad};

fn assert_rejected(result: Result<cratestack_cose::Opened, CratestackError>, what: &str) {
    match result {
        Err(CratestackError::Unauthorized(message)) => assert_eq!(message, UNAUTHENTICATED),
        other => panic!("{what}: expected the coarse 401, got {other:?}"),
    }
}

async fn open(
    alg: CoseAlg,
    body: Vec<u8>,
    bind: &Binding<'_>,
) -> Result<cratestack_cose::Opened, CratestackError> {
    common::server(alg, IAT)
        .open_request(Bytes::from(body), bind)
        .await
}

async fn flip(alg: CoseAlg, at: usize, what: &str) {
    let sealed = common::sealed_request(alg, &rest_request()).await.to_vec();
    let mut tampered = sealed.clone();
    tampered[at] ^= 0x01;
    assert_rejected(open(alg, tampered, &rest_request()).await, what);
    // Control: the untouched message opens, so the rejection is the flip's.
    open(alg, sealed, &rest_request())
        .await
        .expect("control opens");
}

#[tokio::test]
async fn a_bit_flip_in_the_payload_rejects() {
    for &alg in CoseAlg::ALL {
        let sealed = common::sealed_request(alg, &rest_request()).await;
        let payload = layout(&sealed).payload;
        flip(alg, payload.start, "payload first byte").await;
        flip(alg, payload.end - 1, "payload last byte").await;
    }
}

#[tokio::test]
async fn a_bit_flip_in_the_protected_header_rejects() {
    for &alg in CoseAlg::ALL {
        let sealed = common::sealed_request(alg, &rest_request()).await;
        let protected = layout(&sealed).protected;
        // Content is `a3 01 <alg> 04 48 <kid×8> 0f …`: offset 12 is the
        // kid's last byte, 13 the claims label; then the cti's last byte.
        flip(alg, protected.start + 12, "protected kid").await;
        flip(alg, protected.start + 13, "protected claims label").await;
        flip(alg, protected.end - 1, "protected cti").await;
    }
}

#[tokio::test]
async fn a_bit_flip_in_the_signature_rejects() {
    for &alg in CoseAlg::ALL {
        let sealed = common::sealed_request(alg, &rest_request()).await;
        let signature = layout(&sealed).signature;
        flip(alg, signature.start, "signature first byte").await;
        flip(alg, signature.end - 1, "signature last byte").await;
    }
}

/// Exhaustive: every bit of every byte, for the cheap algorithms.
#[tokio::test]
async fn every_single_bit_flip_rejects() {
    for alg in [CoseAlg::Ed25519, CoseAlg::Hmac256_64] {
        let sealed = common::sealed_request(alg, &rest_request()).await.to_vec();
        let server = common::server(alg, IAT);
        for at in 0..sealed.len() {
            for bit in 0..8 {
                let mut tampered = sealed.clone();
                tampered[at] ^= 1 << bit;
                let result = server
                    .open_request(Bytes::from(tampered), &rest_request())
                    .await;
                assert_rejected(result, &format!("{alg:?} byte {at} bit {bit}"));
            }
        }
        server
            .open_request(Bytes::from(sealed), &rest_request())
            .await
            .expect("control opens after 8×len rejections");
    }
}

/// Seal for `rest_request()`, open with one field changed.
async fn aad_mismatch(change: impl Fn(&mut Binding<'static>), what: &str) {
    for &alg in CoseAlg::ALL {
        let sealed = common::sealed_request(alg, &rest_request()).await.to_vec();
        let mut other = rest_request();
        change(&mut other);
        assert_ne!(
            external_aad(&other).expect("aad"),
            external_aad(&rest_request()).expect("aad")
        );
        assert_rejected(open(alg, sealed, &other).await, &format!("{what} {alg:?}"));
    }
}

#[tokio::test]
async fn aad_binds_every_request_field() {
    aad_mismatch(|b| b.method = Cow::Borrowed("POST"), "method").await;
    aad_mismatch(
        |b| b.route = Cow::Borrowed("/accounts/{account_id}/refunds/{id}"),
        "route",
    )
    .await;
    aad_mismatch(
        |b| b.path_params = PathParams::Borrowed(&["acc_42", "pay_8"]),
        "path value",
    )
    .await;
    aad_mismatch(
        |b| b.path_params = PathParams::Borrowed(&["pay_7", "acc_42"]),
        "path order",
    )
    .await;
    aad_mismatch(
        |b| b.path_params = PathParams::Borrowed(&["acc_42pay_7"]),
        "path split",
    )
    .await;
    aad_mismatch(|b| b.path_params = PathParams::EMPTY, "path dropped").await;
    aad_mismatch(|b| b.query = Some(Cow::Borrowed("dry_run=true")), "query").await;
    aad_mismatch(|b| b.query = None, "query dropped").await;
    aad_mismatch(|b| b.schema_sha[31] ^= 0x01, "schema_sha").await;
    aad_mismatch(
        |b| b.payload_media_type = Cow::Borrowed("application/json"),
        "payload_type",
    )
    .await;
}

#[tokio::test]
async fn aad_binds_request_kind_digest_and_status_on_responses() {
    for &alg in CoseAlg::ALL {
        let request = rest_request();
        let request_body = common::sealed_request(alg, &request).await;
        let response = common::response_to(&request, &request_body, 200);
        let sealed = common::server(alg, IAT)
            .seal_response(&payment_bytes(), &response)
            .await
            .expect("seal response");
        let client = common::client(alg, IAT, common::CTI_16);
        client
            .open_response(sealed.clone(), &response)
            .await
            .expect("control opens");

        let mut digest = response.clone();
        digest.response.as_mut().expect("response").request.digest[0] ^= 0x01;
        assert_rejected(
            client.open_response(sealed.clone(), &digest).await,
            "request_digest",
        );
        // The same digest, claimed as the other kind (cratestack#1005,
        // 2026-09-25): see `tests/request_kind.rs` for the attack.
        let mut kind = response.clone();
        kind.response.as_mut().expect("response").request.kind = RequestKind::Unsigned;
        assert_rejected(
            client.open_response(sealed.clone(), &kind).await,
            "request_kind",
        );
        let mut status = response.clone();
        status.response.as_mut().expect("response").status = 201;
        assert_rejected(
            client.open_response(sealed.clone(), &status).await,
            "status",
        );
        // A response to another request with the same shape.
        let other = common::response_to(&request, b"a different request body", 200);
        assert_rejected(client.open_response(sealed, &other).await, "response swap");
    }
}

#[tokio::test]
async fn empty_query_and_no_query_bind_the_same() {
    let sealed = common::sealed_request(CoseAlg::Ed25519, &common::rpc_request())
        .await
        .to_vec();
    let empty = Binding {
        query: Some(Cow::Borrowed("")),
        ..common::rpc_request()
    };
    open(CoseAlg::Ed25519, sealed, &empty)
        .await
        .expect("\"\" and None are both null");
}

/// A Sign1 message re-tagged as Mac0 (and back) rejects in both envelopes.
#[tokio::test]
async fn tag_swap_rejects() {
    let sign1 = common::sealed_request(CoseAlg::Ed25519, &rest_request())
        .await
        .to_vec();
    let mut as_mac0 = sign1.clone();
    as_mac0[0] = TAG_MAC0;
    assert_rejected(
        open(CoseAlg::Ed25519, as_mac0.clone(), &rest_request()).await,
        "18→17 in Sign1",
    );
    assert_rejected(
        open(CoseAlg::Hmac256_256, as_mac0, &rest_request()).await,
        "18→17 in Mac0",
    );

    let mac0 = common::sealed_request(CoseAlg::Hmac256_256, &rest_request())
        .await
        .to_vec();
    let mut as_sign1 = mac0;
    as_sign1[0] = TAG_SIGN1;
    assert_rejected(
        open(CoseAlg::Ed25519, as_sign1, &rest_request()).await,
        "17→18 in Sign1",
    );
}

fn protected_for(alg_id: i8, kid: &[u8]) -> Vec<u8> {
    forge::request_protected(
        alg_id,
        kid,
        u32::try_from(IAT).expect("u32"),
        &common::unhex(common::CTI_16),
    )
}

/// A Sign1 envelope whose resolver ignores `(kid, alg)`: only the opener's
/// own checks stand between the message and acceptance.
fn sloppy_sign1_server(keys: Vec<CoseVerifyKey>) -> cratestack_cose::CoseEnvelope {
    common::server_with(
        CoseAlg::Ed25519,
        IAT,
        Arc::new(FixedResolver(keys)),
        Arc::new(InMemoryNonceStore::new()),
    )
}

/// An HMAC-authenticated message wearing the Sign1 tag: `alg` 4 inside tag
/// 18, MAC'd over the `Signature1` structure. If a Sign1 envelope accepted
/// it, anyone holding a shared service secret could pass as a
/// non-repudiable signer.
#[tokio::test]
async fn hmac_alg_inside_a_sign1_tag_rejects() {
    let hmac = common::hmac(CoseAlg::Hmac256_64);
    let aad = external_aad(&rest_request()).expect("aad");
    let payload = payment_bytes();
    let protected = protected_for(4, hmac.kid());
    let tag = forge::hmac_sign(&forge::sign1_tbs(&protected, &aad, &payload), 8);
    let message = forge::assemble(TAG_SIGN1, &protected, &[0xa0], &payload, &tag);
    let server = sloppy_sign1_server(vec![hmac.verify_key()]);
    assert_rejected(
        server
            .open_request(Bytes::from(message), &rest_request())
            .await,
        "alg 4 in tag 18",
    );
}

/// Algorithm swaps among the signature algorithms: an Ed25519 signature
/// under a header claiming ESP256, and the reverse.
#[tokio::test]
async fn signature_alg_swap_rejects() {
    let aad = external_aad(&rest_request()).expect("aad");
    let payload = payment_bytes();
    let keys = vec![common::ed25519().verify_key(), common::p256().verify_key()];

    let claims_esp256 = protected_for(-9, common::ed25519().kid());
    let ed_sig = forge::ed25519_sign(&forge::sign1_tbs(&claims_esp256, &aad, &payload));
    let message = forge::assemble(TAG_SIGN1, &claims_esp256, &[0xa0], &payload, &ed_sig);
    assert_rejected(
        sloppy_sign1_server(keys.clone())
            .open_request(Bytes::from(message), &rest_request())
            .await,
        "-19 signature under -9",
    );

    let claims_ed25519 = protected_for(-19, common::p256().kid());
    let p_sig = forge::p256_sign(&forge::sign1_tbs(&claims_ed25519, &aad, &payload));
    let message = forge::assemble(TAG_SIGN1, &claims_ed25519, &[0xa0], &payload, &p_sig);
    assert_rejected(
        sloppy_sign1_server(keys)
            .open_request(Bytes::from(message), &rest_request())
            .await,
        "-9 signature under -19",
    );
}

/// The classic confusion: MAC the message with the Ed25519 *public* key
/// as the HMAC secret. A verifier that treated key material as bytes would
/// accept it; the typed key cannot be used that way.
#[tokio::test]
async fn an_ed25519_public_key_is_never_an_hmac_secret() {
    let ed = common::ed25519().verify_key();
    let public = ed.ed25519_bytes().expect("an Ed25519 key");
    let aad = external_aad(&rest_request()).expect("aad");
    let payload = payment_bytes();
    let protected = protected_for(5, &ed.kid());
    let tag = forge::hmac_with(&public, &forge::mac0_tbs(&protected, &aad, &payload), 32);
    let message = forge::assemble(TAG_MAC0, &protected, &[0xa0], &payload, &tag);
    let server = common::server_with(
        CoseAlg::Hmac256_256,
        IAT,
        Arc::new(FixedResolver(vec![ed])),
        Arc::new(InMemoryNonceStore::new()),
    );
    assert_rejected(
        server
            .open_request(Bytes::from(message), &rest_request())
            .await,
        "public key as HMAC secret",
    );
}

/// -8 (EdDSA) and -7 (ES256) are deprecated by RFC 9864 and refused, even
/// with a valid signature by a key the resolver returns.
#[tokio::test]
async fn deprecated_ids_minus_8_and_minus_7_reject() {
    let aad = external_aad(&rest_request()).expect("aad");
    let payload = payment_bytes();
    let keys = vec![common::ed25519().verify_key(), common::p256().verify_key()];

    let eddsa = protected_for(-8, common::ed25519().kid());
    let message = forge::ed25519_request(&eddsa, &aad, &payload);
    assert_rejected(
        sloppy_sign1_server(keys.clone())
            .open_request(Bytes::from(message), &rest_request())
            .await,
        "-8",
    );

    let es256 = protected_for(-7, common::p256().kid());
    let sig = forge::p256_sign(&forge::sign1_tbs(&es256, &aad, &payload));
    let message = forge::assemble(TAG_SIGN1, &es256, &[0xa0], &payload, &sig);
    assert_rejected(
        sloppy_sign1_server(keys.clone())
            .open_request(Bytes::from(message), &rest_request())
            .await,
        "-7",
    );

    // Control: the same construction with -19 is accepted.
    let good = protected_for(-19, common::ed25519().kid());
    let message = forge::ed25519_request(&good, &aad, &payload);
    sloppy_sign1_server(keys)
        .open_request(Bytes::from(message), &rest_request())
        .await
        .expect("-19 control");
}
