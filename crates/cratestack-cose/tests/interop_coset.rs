//! The crate's hand-rolled encoding, checked against `coset` (and, through
//! it, `ciborium`), an independent COSE implementation.
//!
//! If these agree, the wire bytes, the `Sig_structure` / `MAC_structure`
//! the signature covers, and the AAD are what RFC 9052 and ADR 0006 §4 say,
//! not merely what this crate happens to produce consistently.

mod common;

use common::fixture::payment_bytes;
use common::{CTI_16, IAT, rest_request, rpc_request, unhex};
use coset::cbor::value::Value;
use coset::iana::Algorithm;
use coset::{
    CborSerializable, CoseMac0, CoseMac0Builder, CoseSign1, CoseSign1Builder, HeaderBuilder,
    TaggedCborSerializable,
};
use cratestack_core::{Binding, RequestKind};
use cratestack_cose::{CoseAlg, CoseSigner, RequestNonce, external_aad, request_digest_unsigned};
use p256::ecdsa::signature::Verifier as _;

fn coset_alg(alg: CoseAlg) -> Algorithm {
    match alg {
        CoseAlg::Ed25519 => Algorithm::Ed25519,
        CoseAlg::Esp256 => Algorithm::ESP256,
        CoseAlg::Hmac256_64 => Algorithm::HMAC_256_64,
        CoseAlg::Hmac256_256 => Algorithm::HMAC_256_256,
        _ => unreachable!("four algorithms"),
    }
}

/// The AAD as `ciborium` encodes the same array.
fn aad_via_ciborium(bind: &Binding<'_>) -> Vec<u8> {
    let mut items = vec![
        Value::Integer(1.into()),
        Value::Text(bind.audience.to_string()),
        Value::Text(bind.method.to_string()),
        Value::Text(bind.route.to_string()),
        Value::Array(
            bind.path_params
                .iter()
                .map(|value| Value::Text(value.to_owned()))
                .collect(),
        ),
        match bind.query.as_deref() {
            Some(query) if !query.is_empty() => Value::Text(query.to_owned()),
            _ => Value::Null,
        },
        Value::Bytes(bind.schema_sha.to_vec()),
        Value::Text(bind.payload_media_type.to_string()),
        // `bound_headers` (cratestack#1006, S1): always two slots, in order.
        Value::Array(
            [
                &bind.bound_headers.idempotency_key,
                &bind.bound_headers.if_match,
            ]
            .into_iter()
            .map(|value| match value.as_deref() {
                Some(value) => Value::Text(value.to_owned()),
                None => Value::Null,
            })
            .collect(),
        ),
    ];
    if let Some(response) = &bind.response {
        // The ADR's numbers, spelled out here rather than taken from
        // `RequestKind::code`, so this stays an independent encoding.
        let kind: u8 = match response.request.kind {
            RequestKind::Unsigned => 0,
            RequestKind::Signed => 1,
        };
        items.push(Value::Integer(kind.into()));
        items.push(Value::Bytes(response.request.digest.to_vec()));
        items.push(Value::Integer(response.status.into()));
    }
    let mut out = Vec::new();
    coset::cbor::ser::into_writer(&Value::Array(items), &mut out).expect("encode");
    out
}

#[test]
fn aad_matches_ciborium_for_requests_and_responses() {
    for bind in [rpc_request(), rest_request()] {
        assert_eq!(external_aad(&bind).expect("aad"), aad_via_ciborium(&bind));
        let signed = common::response_to(&bind, b"request body", 201);
        let unsigned = common::answering(
            &bind,
            request_digest_unsigned(&RequestNonce::from_bytes([7; 16]), b"request body"),
            201,
        );
        for response in [signed, unsigned] {
            assert_eq!(
                external_aad(&response).expect("aad"),
                aad_via_ciborium(&response)
            );
        }
    }
}

/// Build the message `coset` would build for the same inputs.
async fn coset_request(alg: CoseAlg, bind: &Binding<'_>) -> Vec<u8> {
    let signer = common::signer(alg);
    let header = HeaderBuilder::new()
        .algorithm(coset_alg(alg))
        .key_id(signer.kid().to_vec())
        .value(
            15,
            Value::Map(vec![
                (Value::Integer(6.into()), Value::Integer(IAT.into())),
                (Value::Integer(7.into()), Value::Bytes(unhex(CTI_16))),
            ]),
        )
        .build();
    let aad = external_aad(bind).expect("aad");
    let payload = payment_bytes();
    // `coset`'s builders take a synchronous closure; compute the signature
    // over the structure `coset` itself builds, then attach it.
    let protected = coset::ProtectedHeader {
        original_data: None,
        header: header.clone(),
    }
    .to_vec()
    .expect("protected");
    let tbs = match alg.mode() {
        cratestack_cose::CoseMode::Sign1 => common::forge::sign1_tbs(&protected, &aad, &payload),
        cratestack_cose::CoseMode::Mac0 => common::forge::mac0_tbs(&protected, &aad, &payload),
    };
    let mut signature = signer.sign(&tbs).await.expect("sign");
    if alg == CoseAlg::Esp256 {
        // The envelope's contract, not `coset`'s: an ESP256 signature goes
        // on the wire with a low `s` (RFC 6979 gives either).
        let raw = p256::ecdsa::Signature::from_slice(&signature).expect("signature");
        signature = raw.normalize_s().to_bytes().to_vec();
    }
    match alg.mode() {
        cratestack_cose::CoseMode::Sign1 => CoseSign1Builder::new()
            .protected(header)
            .payload(payload)
            .create_signature(&aad, |_| signature.clone())
            .build()
            .to_tagged_vec()
            .expect("sign1"),
        cratestack_cose::CoseMode::Mac0 => CoseMac0Builder::new()
            .protected(header)
            .payload(payload)
            .create_tag(&aad, |_| signature.clone())
            .build()
            .to_tagged_vec()
            .expect("mac0"),
    }
}

#[tokio::test]
async fn sealed_requests_are_byte_identical_to_coset() {
    // ESP256 is included: the in-process signer is RFC 6979 deterministic.
    for &alg in CoseAlg::ALL {
        for bind in [rpc_request(), rest_request()] {
            let ours = common::sealed_request(alg, &bind).await;
            let theirs = coset_request(alg, &bind).await;
            assert_eq!(ours.to_vec(), theirs, "{alg:?}");
        }
    }
}

#[tokio::test]
async fn coset_parses_and_verifies_what_the_crate_seals() {
    let bind = rest_request();
    let aad = external_aad(&bind).expect("aad");
    let payload = payment_bytes();

    let sign1 =
        CoseSign1::from_tagged_slice(&common::sealed_request(CoseAlg::Ed25519, &bind).await)
            .expect("coset parses Sign1");
    assert_eq!(sign1.payload.as_deref(), Some(payload.as_slice()));
    assert_eq!(sign1.protected.header.key_id, common::ed25519().kid());
    let key = ed25519_dalek::VerifyingKey::from(&ed25519_dalek::SigningKey::from_bytes(
        &common::ED25519_SEED,
    ));
    sign1
        .verify_signature(&aad, |sig, data| {
            key.verify_strict(data, &ed25519_dalek::Signature::from_slice(sig)?)
        })
        .expect("coset-driven Ed25519 verification");

    let es = CoseSign1::from_tagged_slice(&common::sealed_request(CoseAlg::Esp256, &bind).await)
        .expect("coset parses ESP256 Sign1");
    let p256_key =
        p256::ecdsa::SigningKey::from_slice(&common::unhex32(common::P256_SCALAR)).expect("scalar");
    es.verify_signature(&aad, |sig, data| {
        p256_key
            .verifying_key()
            .verify(data, &p256::ecdsa::Signature::from_slice(sig)?)
    })
    .expect("coset-driven ESP256 verification");

    let mac0 =
        CoseMac0::from_tagged_slice(&common::sealed_request(CoseAlg::Hmac256_64, &bind).await)
            .expect("coset parses Mac0");
    mac0.verify_payload_tag(
        &aad,
        || "no payload",
        |tag, data| {
            if tag == common::forge::hmac_sign(data, 8) {
                Ok(())
            } else {
                Err("bad tag")
            }
        },
    )
    .expect("coset-driven HMAC 256/64 verification");
}
