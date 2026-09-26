//! `Cratestack-Nonce` (maintainer decision on cratestack#1005,
//! 2026-09-24): a signed response to an unsigned request is bound to the
//! client's 16-byte nonce, so it answers exactly one request.

mod common;

use std::borrow::Cow;

use common::{CTI_16, IAT, rest_request};
use cratestack_core::{Binding, CratestackError, RequestKind};
use cratestack_cose::{
    CoseAlg, CoseEnvelope, CoseMode, NONCE_HEADER, NONCE_HEADER_VALUE_LEN, RequestNonce,
    UNAUTHENTICATED, request_digest_unsigned,
};
use sha2::{Digest, Sha256};

fn get() -> Binding<'static> {
    Binding {
        method: Cow::Borrowed("GET"),
        query: None,
        ..rest_request()
    }
}

fn answering(nonce: &RequestNonce) -> Binding<'static> {
    common::answering(&get(), request_digest_unsigned(nonce, b""), 200)
}

/// Two `GET`s of one URL, each with its own nonce: the signed answer to
/// the first does not verify as the answer to the second. Without the
/// nonce both digests were SHA-256 of the empty body, and one signed
/// response answered every `GET` of that URL, forever.
#[tokio::test]
async fn a_response_to_one_get_does_not_answer_another() {
    for &alg in CoseAlg::ALL {
        let first = RequestNonce::from_bytes([0x11; 16]);
        let second = RequestNonce::from_bytes([0x22; 16]);
        let sealed = common::server(alg, IAT)
            .seal_response(b"\xa1gbalance\x19\x03\xe8", &answering(&first))
            .await
            .expect("seal");
        let client = common::client(alg, IAT, CTI_16);
        match client
            .open_response(sealed.clone(), &answering(&second))
            .await
        {
            Err(CratestackError::Unauthorized(message)) => assert_eq!(message, UNAUTHENTICATED),
            other => panic!("{alg:?}: answered another GET: {other:?}"),
        }
        client
            .open_response(sealed, &answering(&first))
            .await
            .expect("it answers its own GET");
    }
}

#[test]
fn the_unsigned_digest_is_sha256_of_nonce_then_payload() {
    let nonce = RequestNonce::from_bytes(*b"0123456789abcdef");
    let mut expected = Sha256::new();
    expected.update(b"0123456789abcdef");
    expected.update(b"{\"amount\":1}");
    let expected: [u8; 32] = expected.finalize().into();
    let digest = request_digest_unsigned(&nonce, b"{\"amount\":1}");
    assert_eq!(digest.digest, expected);
    assert_eq!(digest.kind, RequestKind::Unsigned);
    // Empty payload (a GET): the nonce alone.
    let empty: [u8; 32] = Sha256::digest(b"0123456789abcdef").into();
    assert_eq!(request_digest_unsigned(&nonce, b"").digest, empty);
    // It is not the signed-request digest of the same bytes.
    assert_ne!(
        request_digest_unsigned(&nonce, b"").digest,
        cratestack_cose::request_digest(b"").digest
    );
    assert_eq!(
        cratestack_cose::request_digest(b"").kind,
        RequestKind::Signed
    );
}

#[test]
fn the_header_is_22_base64url_characters() {
    assert_eq!(NONCE_HEADER, "Cratestack-Nonce");
    let nonce = RequestNonce::from_bytes([0xfb; 16]);
    let value = nonce.to_header_value();
    assert_eq!(value.len(), NONCE_HEADER_VALUE_LEN);
    assert_eq!(value, "-_v7-_v7-_v7-_v7-_v7-w");
    assert_eq!(
        RequestNonce::from_header_value(value.as_bytes()).expect("parse"),
        nonce
    );
    assert_eq!(
        RequestNonce::from_header(Some(value.as_bytes())).expect("parse"),
        nonce
    );
}

fn is_bad_request(result: Result<RequestNonce, CratestackError>) -> bool {
    matches!(result, Err(CratestackError::BadRequest(_)))
}

#[test]
fn a_missing_or_malformed_nonce_is_an_error() {
    let good = RequestNonce::from_bytes([0xfb; 16]).to_header_value();
    assert!(is_bad_request(RequestNonce::from_header(None)), "missing");
    for bad in [
        String::new(),
        good[..21].to_owned(),               // too short
        format!("{good}A"),                  // too long
        format!("{good}=="),                 // padded
        format!("+{}", &good[1..]),          // standard alphabet
        format!("/{}", &good[1..]),          // standard alphabet
        format!(" {}", &good[1..]),          // whitespace
        format!("{}x", &good[..21]),         // non-zero trailing bits
        format!("{}B", &good[..21]),         // non-zero trailing bits
        "AAAAAAAAAAAAAAAAAAAAA=".to_owned(), // padding inside 22
    ] {
        assert!(
            is_bad_request(RequestNonce::from_header_value(bad.as_bytes())),
            "{bad:?} accepted"
        );
    }
    // The error says only that the header is malformed.
    let error = RequestNonce::from_header_value(b"nope").expect_err("bad");
    assert_eq!(error.public_message(), "malformed Cratestack-Nonce header");
}

#[test]
fn the_envelope_draws_nonces_from_its_injectable_source() {
    let pinned = CoseEnvelope::client(
        CoseMode::Sign1,
        common::signer(CoseAlg::Ed25519),
        common::resolver(),
    )
    .nonce_source(|| Ok([0x42; 16]))
    .build()
    .expect("client");
    assert_eq!(
        pinned.request_nonce().expect("nonce").as_bytes(),
        &[0x42; 16]
    );

    let failing = CoseEnvelope::client(
        CoseMode::Sign1,
        common::signer(CoseAlg::Ed25519),
        common::resolver(),
    )
    .nonce_source(|| Err(CratestackError::Internal("rng down".to_owned())))
    .build()
    .expect("client");
    assert!(matches!(
        failing.request_nonce(),
        Err(CratestackError::Internal(_))
    ));

    let default = common::client(CoseAlg::Ed25519, IAT, CTI_16);
    let (a, b) = (
        default.request_nonce().expect("a"),
        default.request_nonce().expect("b"),
    );
    assert_ne!(a, b, "the default source is random");
    assert_ne!(cratestack_cose::random_request_nonce().expect("random"), a);
}
