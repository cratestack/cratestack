//! `ExternalSigner`: a key outside the process, whose keystore answers with
//! DER (Android Keystore, iOS `SecKey`) or with raw `r ‖ s` (cratestack#1007).

mod common;

use std::sync::Arc;

use common::{P256_SCALAR, rest_request, unhex32};
use cratestack_core::CratestackError;
use cratestack_cose::{CoseAlg, CoseEnvelope, CoseMode, CoseSigner, ExternalSigner};
use p256::ecdsa::signature::Signer as _;
use p256::ecdsa::{Signature, SigningKey};

fn keystore() -> SigningKey {
    SigningKey::from_slice(&unhex32(P256_SCALAR)).expect("valid scalar")
}

fn public_sec1() -> Vec<u8> {
    common::p256()
        .verify_key()
        .p256_sec1_uncompressed()
        .expect("P-256 key")
        .to_vec()
}

/// A keystore that hashes with SHA-256 itself (SHA256withECDSA) and answers
/// in DER, as both mobile platforms do.
fn der_signer() -> ExternalSigner {
    let key = keystore();
    ExternalSigner::esp256(&public_sec1(), move |tbs| {
        let key = key.clone();
        async move {
            let signature: Signature = key.sign(&tbs);
            Ok(signature.to_der().as_bytes().to_vec())
        }
    })
    .expect("signer")
}

fn envelope(signer: ExternalSigner) -> CoseEnvelope {
    CoseEnvelope::client(CoseMode::Sign1, Arc::new(signer), common::resolver())
        .build()
        .expect("client envelope")
}

#[test]
fn the_kid_is_the_thumbprint_of_the_public_key() {
    let external = der_signer();
    assert_eq!(external.alg(), CoseAlg::Esp256);
    assert_eq!(external.kid(), common::p256().kid());
}

#[tokio::test]
async fn a_der_signature_is_converted_and_verifies_at_the_server() {
    let bind = rest_request();
    let sealed = envelope(der_signer())
        .seal_request(b"\xa0", &bind)
        .await
        .expect("seal with a DER keystore");
    let server = common::server(CoseAlg::Esp256, common::now());
    let opened = server
        .open_request(sealed, &bind)
        .await
        .expect("the server verifies what the keystore signed");
    assert_eq!(opened.payload.as_ref(), b"\xa0");
}

#[tokio::test]
async fn a_raw_signature_is_accepted_too() {
    let key = keystore();
    let signer = ExternalSigner::esp256(&public_sec1(), move |tbs| {
        let key = key.clone();
        async move {
            let signature: Signature = key.sign(&tbs);
            Ok(signature.to_bytes().to_vec())
        }
    })
    .expect("signer");
    let bind = rest_request();
    let sealed = envelope(signer)
        .seal_request(b"\xa0", &bind)
        .await
        .expect("seal");
    common::server(CoseAlg::Esp256, common::now())
        .open_request(sealed, &bind)
        .await
        .expect("verifies");
}

#[tokio::test]
async fn the_keystore_receives_the_full_to_be_signed_bytes() {
    // Not a digest: a 32-byte input would mean the envelope pre-hashed and
    // the keystore's own SHA-256 would then hash twice.
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let key = keystore();
    let sink = seen.clone();
    let signer = ExternalSigner::esp256(&public_sec1(), move |tbs| {
        let key = key.clone();
        let sink = sink.clone();
        async move {
            let signature: Signature = key.sign(&tbs);
            sink.lock().unwrap().push(tbs);
            Ok(signature.to_der().as_bytes().to_vec())
        }
    })
    .expect("signer");
    envelope(signer)
        .seal_request(b"\xa0", &rest_request())
        .await
        .expect("seal");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    // `["Signature1", protected, external_aad, payload]`.
    assert!(seen[0].starts_with(&[0x84, 0x6a, b'S', b'i', b'g']));
}

#[tokio::test]
async fn a_signature_that_is_neither_der_nor_raw_is_refused() {
    for garbage in [
        vec![],
        vec![0u8; 63],
        vec![0x30, 0x02, 0x01, 0x01],
        vec![7; 65],
    ] {
        let signer = ExternalSigner::esp256(&public_sec1(), move |_tbs| {
            let garbage = garbage.clone();
            async move { Ok(garbage) }
        })
        .expect("signer");
        let error = envelope(signer)
            .seal_request(b"\xa0", &rest_request())
            .await
            .expect_err("a malformed signature never reaches the wire");
        assert!(matches!(error, CratestackError::Internal(_)), "{error:?}");
    }
}

#[tokio::test]
async fn a_keystore_failure_is_a_backend_error_not_a_signature() {
    let signer = ExternalSigner::esp256(&public_sec1(), |_tbs| async {
        Err(CratestackError::Unavailable("keystore locked".to_owned()))
    })
    .expect("signer");
    let error = envelope(signer)
        .seal_request(b"\xa0", &rest_request())
        .await
        .expect_err("locked keystore");
    assert!(matches!(error, CratestackError::Internal(_)), "{error:?}");
}

#[test]
fn a_public_key_that_is_not_on_the_curve_is_refused() {
    let error =
        ExternalSigner::esp256(&[4; 65], |_tbs| async { Ok(Vec::new()) }).expect_err("not a point");
    assert!(matches!(error, CratestackError::Validation(_)), "{error:?}");
}

/// Fixed DER encodings a keystore may return, each converted to the 64-byte
/// `r ‖ s` COSE carries.
mod der_vectors {
    use super::*;

    async fn raw_of(answer: Vec<u8>) -> Vec<u8> {
        let signer = ExternalSigner::esp256(&public_sec1(), move |_tbs| {
            let answer = answer.clone();
            async move { Ok(answer) }
        })
        .expect("signer");
        signer.sign(b"tbs").await.expect("converted")
    }

    fn padded(value: &[u8]) -> Vec<u8> {
        let mut out = vec![0; 32 - value.len()];
        out.extend_from_slice(value);
        out
    }

    #[tokio::test]
    async fn a_high_bit_r_and_s_lose_their_zero_padding() {
        // INTEGERs whose top bit is set carry a leading 0x00 in DER: 33 bytes.
        let (mut r, mut s) = (vec![0x80], vec![0xC0]);
        r.extend([0x11; 31]);
        s.extend([0x22; 31]);
        let mut der = vec![0x30, 0x46, 0x02, 0x21, 0x00];
        der.extend(&r);
        der.extend([0x02, 0x21, 0x00]);
        der.extend(&s);
        assert_eq!(raw_of(der).await, [r, s].concat());
    }

    #[tokio::test]
    async fn a_short_r_and_s_are_left_padded_to_32_bytes() {
        // r = 0x0102 and s = 0x7f: DER drops the leading zeros.
        let der = vec![0x30, 0x07, 0x02, 0x02, 0x01, 0x02, 0x02, 0x01, 0x7f];
        assert_eq!(
            raw_of(der).await,
            [padded(&[0x01, 0x02]), padded(&[0x7f])].concat()
        );
    }

    #[tokio::test]
    async fn garbage_is_refused() {
        let signer = ExternalSigner::esp256(&public_sec1(), |_tbs| async { Ok(vec![0x30, 0x01]) })
            .expect("signer");
        assert!(matches!(
            signer.sign(b"tbs").await,
            Err(CratestackError::Internal(_))
        ));
    }
}

#[test]
fn a_compressed_sec1_key_gives_the_same_kid() {
    // SEC1 compression by hand: the parity of y picks 0x02 or 0x03, then x.
    let uncompressed = public_sec1();
    let mut compressed = vec![0x02 | (uncompressed[64] & 1)];
    compressed.extend_from_slice(&uncompressed[1..33]);
    assert_eq!(compressed.len(), 33);
    let signer = ExternalSigner::esp256(&compressed, |_tbs| async { Ok(Vec::new()) })
        .expect("a compressed key is accepted");
    assert_eq!(signer.kid(), der_signer().kid());
}
