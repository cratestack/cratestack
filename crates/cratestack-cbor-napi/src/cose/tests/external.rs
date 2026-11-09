//! The two-step seal with an external signer (issue #1010): the signer is
//! stood in for by an in-memory key, called exactly as a WebCrypto key
//! would be, and the sealed bytes are the shared vectors'.

use cratestack_cose::{CoseSigner, Ed25519Signer, P256Signer};

use super::super::drive::ready;
use super::super::envelope::Envelope;
use super::support::*;

fn sign(keys: &serde_json::Value, key: &str, to_be_signed: &[u8]) -> Vec<u8> {
    let entry = &keys[key];
    let signed = if key == "p256" {
        let scalar: [u8; 32] = hex_field(entry, "scalar").try_into().unwrap();
        ready(P256Signer::from_scalar(&scalar).unwrap().sign(to_be_signed))
    } else {
        let seed: [u8; 32] = hex_field(entry, "seed").try_into().unwrap();
        ready(Ed25519Signer::from_seed(&seed).sign(to_be_signed))
    };
    signed.expect("ready").expect("signature")
}

fn public(keys: &serde_json::Value, key: &str) -> Vec<u8> {
    let entry = &keys[key];
    hex_field(
        entry,
        if key == "p256" {
            "public_sec1_uncompressed"
        } else {
            "public"
        },
    )
}

fn external(keys: &serde_json::Value, key: &str, pin: Option<(u64, &str)>) -> Envelope {
    let alg = alg_name(&keys[key]);
    Envelope::external(
        alg,
        &public(keys, key),
        &server_keys(keys, key),
        AUDIENCE,
        options(pin),
    )
    .unwrap_or_else(|e| panic!("{e:?}"))
}

/// ASN.1 DER of a raw `r ‖ s`, which is what a platform keystore returns.
fn der(raw: &[u8]) -> Vec<u8> {
    let integer = |bytes: &[u8]| {
        let start = bytes
            .iter()
            .position(|b| *b != 0)
            .unwrap_or(bytes.len() - 1);
        let mut out = vec![0x02];
        let digits = &bytes[start..];
        let pad = usize::from(digits[0] & 0x80 != 0);
        out.push((digits.len() + pad) as u8);
        out.extend(std::iter::repeat_n(0, pad));
        out.extend_from_slice(digits);
        out
    };
    let body = [integer(&raw[..32]), integer(&raw[32..])].concat();
    [vec![0x30, body.len() as u8], body].concat()
}

#[test]
fn an_external_signer_seals_every_sign1_vector_byte_for_byte() {
    let (keys, unary) = (keys(), unary());
    let mut sealed_count = 0;
    for case in cases(&unary, "request") {
        let key = case["key"].as_str().unwrap();
        if !matches!(key, "p256" | "ed25519") {
            continue;
        }
        let pin = (case["iat"].as_u64().unwrap(), case["cti"].as_str().unwrap());
        let envelope = external(&keys, key, Some(pin));
        let pending = envelope
            .begin_seal(&hex_field(&case, "payload"), &binding(&case["binding"]))
            .unwrap_or_else(|e| panic!("{}: {e:?}", case["name"]));
        let signature = sign(&keys, key, pending.to_be_signed());
        // A platform keystore answers DER for ECDSA: the same seal.
        let signature = if key == "p256" {
            der(&signature)
        } else {
            signature
        };
        let sealed = pending.finish(signature).unwrap();
        assert_eq!(sealed, hex_field(&case, "cose"), "{}", case["name"]);
        sealed_count += 1;
    }
    assert_eq!(sealed_count, 9);
}

#[test]
fn a_signature_that_does_not_verify_is_refused_and_a_seal_finishes_once() {
    let (keys, unary) = (keys(), unary());
    let case = &cases(&unary, "request")[0];
    let bound = binding(&case["binding"]);
    let payload = hex_field(case, "payload");
    let envelope = external(&keys, "ed25519", None);

    let pending = envelope.begin_seal(&payload, &bound).unwrap();
    // A signature over other bytes, and one of the wrong length.
    for bad in [sign(&keys, "ed25519", b"not what was asked"), vec![0; 63]] {
        let again = envelope.begin_seal(&payload, &bound).unwrap();
        let error = again.finish(bad).expect_err("refused");
        assert_eq!(error.code, "misuse");
    }
    let good = sign(&keys, "ed25519", pending.to_be_signed());
    assert!(pending.finish(good.clone()).is_ok());
    let twice = pending.finish(good).expect_err("once");
    assert_eq!(twice.code, "misuse");
}

#[test]
fn the_two_steps_belong_to_an_external_signer_only() {
    let (keys, unary) = (keys(), unary());
    let case = &cases(&unary, "request")[0];
    let bound = binding(&case["binding"]);
    let payload = hex_field(case, "payload");
    let in_memory = envelope(&keys, "ed25519", None).unwrap();
    assert_eq!(
        in_memory.begin_seal(&payload, &bound).err().unwrap().code,
        "misuse"
    );
    let outside = external(&keys, "ed25519", None);
    assert_eq!(
        outside.seal_request(&payload, &bound).err().unwrap().code,
        "misuse"
    );
    // Concurrent seals do not share a signature.
    let (first, second) = (
        outside.begin_seal(&payload, &bound).unwrap(),
        outside.begin_seal(&payload, &bound).unwrap(),
    );
    // Each seal waits on its own signature: the other's (over another random
    // `cti`) does not verify, and the first is still finishable after.
    assert_ne!(first.to_be_signed(), second.to_be_signed());
    let second_signature = sign(&keys, "ed25519", second.to_be_signed());
    assert!(second.finish(second_signature.clone()).is_ok());
    let crossed = first.finish(second_signature);
    assert_eq!(crossed.err().unwrap().code, "misuse");
    assert_eq!(outside.kid(), hex_field(&keys["ed25519"], "kid"));
}

#[test]
fn an_external_signer_of_another_algorithm_or_a_short_key_is_misuse() {
    let keys = keys();
    for (alg, public) in [
        ("hmac256-64", vec![0u8; 32]),
        ("ed25519", vec![0u8; 31]),
        ("rsa", vec![]),
    ] {
        let error = Envelope::external(
            alg,
            &public,
            &server_keys(&keys, "ed25519"),
            "a",
            options(None),
        )
        .expect_err(alg);
        assert_eq!(error.code, "misuse", "{alg}");
    }
}
