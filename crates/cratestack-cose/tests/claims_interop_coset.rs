//! Claim objects against `coset`, an independent COSE implementation (the
//! dev-dependency `interop_coset.rs` already uses), in both directions:
//!
//! - `coset` builds the same message from the same inputs, byte for byte,
//!   signing the `Sig_structure` it computes itself;
//! - `coset` parses what `sign_claims` writes and reads the header back,
//!   and an independent signature library verifies it; and a message
//!   `coset` built and signed opens with `open_claims`.
//!
//! RFC 9052 Appendix C and RFC 8392 Appendix A were checked and do not
//! apply: their examples use the deprecated `-7` / `-8` algorithm ids
//! (refused here), put the `kid` in the unprotected header, or carry the
//! CWT claims in the payload, none of which this profile accepts. It has no
//! published vector, so `coset` is the independent check.

mod common;

use coset::cbor::value::Value;
use coset::iana::Algorithm;
use coset::{CoseSign1, CoseSign1Builder, HeaderBuilder, Label, TaggedCborSerializable};
use cratestack_cose::{CoseAlg, CoseSigner, CwtClaims, open_claims, sign_claims};
use ed25519_dalek::Signer as _;
use p256::ecdsa::signature::Verifier as _;

const TYP: &str = "application/vnd.cratestack.test-grant+cwt";
const AAD: &[u8] = b"tenant-1|grant";
const PAYLOAD: &[u8] = b"grant: 5 sheep";

fn claim_sets() -> Vec<CwtClaims> {
    vec![
        CwtClaims::default(),
        CwtClaims {
            exp: Some(1_790_003_600),
            ..CwtClaims::default()
        },
        CwtClaims {
            aud: Some("vaam".to_owned()),
            exp: Some(1_790_003_600),
            iat: Some(1_790_000_000),
            cti: Some(vec![0xa5; 16]),
        },
    ]
}

/// The CWT claims map, built with `ciborium` values, labels ascending.
fn claims_value(claims: &CwtClaims) -> Option<Value> {
    let int = |n: u64| Value::Integer(n.into());
    let mut entries = Vec::new();
    if let Some(aud) = &claims.aud {
        entries.push((int(3), Value::Text(aud.clone())));
    }
    if let Some(exp) = claims.exp {
        entries.push((int(4), int(exp)));
    }
    if let Some(iat) = claims.iat {
        entries.push((int(6), int(iat)));
    }
    if let Some(cti) = &claims.cti {
        entries.push((int(7), Value::Bytes(cti.clone())));
    }
    (!entries.is_empty()).then_some(Value::Map(entries))
}

fn coset_header(alg: Algorithm, kid: &[u8], claims: &CwtClaims) -> coset::Header {
    let mut builder = HeaderBuilder::new().algorithm(alg).key_id(kid.to_vec());
    if let Some(value) = claims_value(claims) {
        builder = builder.value(15, value);
    }
    builder.value(16, Value::Text(TYP.to_owned())).build()
}

fn sign_ed25519(data: &[u8]) -> Vec<u8> {
    ed25519_dalek::SigningKey::from_bytes(&common::ED25519_SEED)
        .sign(data)
        .to_bytes()
        .to_vec()
}

fn sign_p256_low_s(data: &[u8]) -> Vec<u8> {
    let key =
        p256::ecdsa::SigningKey::from_slice(&common::unhex32(common::P256_SCALAR)).expect("scalar");
    let signature: p256::ecdsa::Signature = key.sign(data);
    signature.normalize_s().to_bytes().to_vec()
}

/// What `coset` builds for the inputs, signing with an independent library.
fn coset_object(alg: CoseAlg, claims: &CwtClaims) -> Vec<u8> {
    let (coset_alg, signer, sign): (_, _, fn(&[u8]) -> Vec<u8>) = match alg {
        CoseAlg::Ed25519 => (Algorithm::Ed25519, common::signer(alg), sign_ed25519),
        CoseAlg::Esp256 => (Algorithm::ESP256, common::signer(alg), sign_p256_low_s),
        _ => unreachable!("signature algorithms only"),
    };
    CoseSign1Builder::new()
        .protected(coset_header(coset_alg, signer.kid(), claims))
        .payload(PAYLOAD.to_vec())
        .create_signature(AAD, sign)
        .build()
        .to_tagged_vec()
        .expect("coset encodes")
}

#[tokio::test]
async fn coset_builds_the_same_bytes() {
    for alg in [CoseAlg::Ed25519, CoseAlg::Esp256] {
        for claims in claim_sets() {
            let ours = sign_claims(common::signer(alg).as_ref(), TYP, &claims, PAYLOAD, AAD)
                .await
                .expect("sign");
            assert_eq!(
                common::hex(&ours),
                common::hex(&coset_object(alg, &claims)),
                "{alg:?} {claims:?}"
            );
        }
    }
}

#[tokio::test]
async fn open_claims_opens_what_coset_built_and_signed() {
    let resolver = common::resolver();
    for alg in [CoseAlg::Ed25519, CoseAlg::Esp256] {
        for claims in claim_sets() {
            let opened = open_claims(resolver.as_ref(), &coset_object(alg, &claims), TYP, AAD)
                .await
                .expect("open");
            assert_eq!(opened.claims, claims);
            assert_eq!(opened.payload, PAYLOAD);
            assert_eq!(opened.alg, alg);
        }
    }
}

#[tokio::test]
async fn coset_parses_and_an_independent_library_verifies_what_the_crate_signs() {
    let ed_key = ed25519_dalek::VerifyingKey::from(&ed25519_dalek::SigningKey::from_bytes(
        &common::ED25519_SEED,
    ));
    let p256_key =
        p256::ecdsa::SigningKey::from_slice(&common::unhex32(common::P256_SCALAR)).expect("scalar");
    for claims in claim_sets() {
        // Ed25519
        let bytes = sign_claims(&common::ed25519(), TYP, &claims, PAYLOAD, AAD)
            .await
            .unwrap();
        let parsed = CoseSign1::from_tagged_slice(&bytes).expect("coset parses");
        assert_header(
            &parsed,
            Algorithm::Ed25519,
            common::ed25519().kid(),
            &claims,
        );
        parsed
            .verify_signature(AAD, |sig, data| {
                ed_key.verify_strict(data, &ed25519_dalek::Signature::from_slice(sig)?)
            })
            .expect("independent Ed25519 verification");
        // ESP256
        let bytes = sign_claims(&common::p256(), TYP, &claims, PAYLOAD, AAD)
            .await
            .unwrap();
        let parsed = CoseSign1::from_tagged_slice(&bytes).expect("coset parses");
        assert_header(&parsed, Algorithm::ESP256, common::p256().kid(), &claims);
        parsed
            .verify_signature(AAD, |sig, data| {
                p256_key
                    .verifying_key()
                    .verify(data, &p256::ecdsa::Signature::from_slice(sig)?)
            })
            .expect("independent ESP256 verification");
    }
}

fn assert_header(parsed: &CoseSign1, alg: Algorithm, kid: &[u8], claims: &CwtClaims) {
    let header = &parsed.protected.header;
    assert_eq!(
        header.alg,
        Some(coset::RegisteredLabelWithPrivate::Assigned(alg))
    );
    assert_eq!(header.key_id, kid);
    assert!(parsed.unprotected.is_empty(), "unprotected header is empty");
    assert_eq!(parsed.payload.as_deref(), Some(PAYLOAD));
    let rest: Vec<_> = header.rest.iter().collect();
    let typ = (Label::Int(16), Value::Text(TYP.to_owned()));
    match claims_value(claims) {
        Some(value) => assert_eq!(rest, vec![&(Label::Int(15), value), &typ]),
        None => assert_eq!(rest, vec![&typ]),
    }
}
