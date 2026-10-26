//! Malformed claim objects: every one is refused, none panics.
//!
//! The header cases are *correctly signed* (the to-be-signed bytes come
//! from `coset`, independent of the crate), so a refusal can only be the
//! parser's strictness, never a broken signature. A positive control per
//! shape proves the hand encoding itself is acceptable.

mod common;

use common::forge::{TAG_MAC0, TAG_SIGN1, assemble, bstr, ed25519_sign, hmac_sign, layout};
use common::forge::{mac0_tbs, sign1_tbs, with_unprotected};
use cratestack_cose::{ClaimsError, CwtClaims, open_claims, sign_claims};

const TYP: &str = "t";
const AAD: &[u8] = b"aad";
const PAYLOAD: &[u8] = b"payload";

const ALG: &[u8] = &[0x01, 0x32]; // 1: -19
const TYP_PART: &[u8] = &[0x10, 0x61, b't']; // 16: "t"

fn kid() -> Vec<u8> {
    [&[0x04, 0x48][..], common::ed25519().kid()].concat()
}

fn hmac_kid() -> Vec<u8> {
    [
        &[0x04, 0x48][..],
        common::hmac(cratestack_cose::CoseAlg::Hmac256_256).kid(),
    ]
    .concat()
}

use cratestack_cose::CoseSigner as _;

/// A definite map head for `parts.len()` entries, then the entries.
fn map(parts: &[&[u8]]) -> Vec<u8> {
    let mut out = vec![0xa0 | u8::try_from(parts.len()).expect("small")];
    for part in parts {
        out.extend_from_slice(part);
    }
    out
}

/// Label 15 with the given claims map contents (entries already encoded).
fn claims(entries: &[&[u8]]) -> Vec<u8> {
    let mut out = vec![0x0f];
    out.extend(map(entries));
    out
}

/// A correctly signed (Ed25519) tag-18 message over `protected`.
fn signed(protected: &[u8]) -> Vec<u8> {
    let sig = ed25519_sign(&sign1_tbs(protected, AAD, PAYLOAD));
    assemble(TAG_SIGN1, protected, &[0xa0], PAYLOAD, &sig)
}

async fn open(message: &[u8]) -> Result<cratestack_cose::OpenedClaims, ClaimsError> {
    open_claims(common::resolver().as_ref(), message, TYP, AAD).await
}

#[tokio::test]
async fn positive_controls_open() {
    let k = kid();
    let all = claims(&[
        &[0x03, 0x61, b'a'],
        &[0x04, 0x01],
        &[0x06, 0x02],
        &[0x07, 0x41, 0x03],
    ]);
    for protected in [
        map(&[ALG, &k, TYP_PART]),
        map(&[ALG, &k, &claims(&[&[0x04, 0x18, 0x18]]), TYP_PART]),
        map(&[ALG, &k, &all, TYP_PART]),
    ] {
        open(&signed(&protected))
            .await
            .expect("hand-encoded header opens");
    }
    let opened = open(&signed(&map(&[ALG, &k, &all, TYP_PART])))
        .await
        .unwrap();
    assert_eq!(
        opened.claims,
        CwtClaims {
            aud: Some("a".into()),
            exp: Some(1),
            iat: Some(2),
            cti: Some(vec![3]),
        }
    );
    // And our signer writes exactly the hand encoding.
    let ours = sign_claims(&common::ed25519(), TYP, &opened.claims, PAYLOAD, AAD)
        .await
        .unwrap();
    assert_eq!(ours, signed(&map(&[ALG, &k, &all, TYP_PART])));
}

#[tokio::test]
async fn malformed_headers_are_refused() {
    let k = kid();
    let ok_claims = claims(&[&[0x04, 0x01]]);
    let long_typ = [&[0x10, 0x78, 129][..], &[b't'; 129]].concat();
    let long_cti = [&[0x0f, 0xa1, 0x07, 0x58, 65][..], &[1; 65]].concat();
    let long_aud = [&[0x0f, 0xa1, 0x03, 0x79, 0x01, 0x01][..], &[b'a'; 257]].concat();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty header", map(&[])),
        ("unknown label 5", map(&[ALG, &k, &[0x05, 0x00], TYP_PART])),
        (
            "label 17 after typ",
            map(&[ALG, &k, TYP_PART, &[0x11, 0x00]]),
        ),
        ("typ before claims", map(&[ALG, &k, TYP_PART, &ok_claims])),
        ("duplicate typ", map(&[ALG, &k, TYP_PART, TYP_PART])),
        ("duplicate alg", map(&[ALG, ALG, &k, TYP_PART])),
        ("kid before alg", map(&[&k, ALG, TYP_PART])),
        ("no typ, no claims", map(&[ALG, &k])),
        (
            "no typ: the envelope request shape",
            map(&[ALG, &k, &claims(&[&[0x06, 0x01], &[0x07, 0x41, 0x01]])]),
        ),
        ("typ is a uint", map(&[ALG, &k, &[0x10, 0x01]])),
        ("typ is empty", map(&[ALG, &k, &[0x10, 0x60]])),
        ("typ is a bstr", map(&[ALG, &k, &[0x10, 0x41, b't']])),
        (
            "typ is indefinite",
            map(&[ALG, &k, &[0x10, 0x7f, 0x61, b't', 0xff]]),
        ),
        (
            "typ length not minimal",
            map(&[ALG, &k, &[0x10, 0x78, 0x01, b't']]),
        ),
        ("typ is not UTF-8", map(&[ALG, &k, &[0x10, 0x61, 0xff]])),
        ("typ too long", map(&[ALG, &k, &long_typ])),
        ("claims empty", map(&[ALG, &k, &[0x0f, 0xa0], TYP_PART])),
        ("claims not a map", map(&[ALG, &k, &[0x0f, 0x80], TYP_PART])),
        (
            "claims indefinite",
            map(&[ALG, &k, &[0x0f, 0xbf, 0x04, 0x01, 0xff], TYP_PART]),
        ),
        (
            "claims map head not minimal",
            map(&[ALG, &k, &[0x0f, 0xb8, 0x01, 0x04, 0x01], TYP_PART]),
        ),
        (
            "claim iss (1)",
            map(&[ALG, &k, &claims(&[&[0x01, 0x61, b'x']]), TYP_PART]),
        ),
        (
            "claim nbf (5)",
            map(&[ALG, &k, &claims(&[&[0x05, 0x01]]), TYP_PART]),
        ),
        (
            "claims out of order",
            map(&[
                ALG,
                &k,
                &claims(&[&[0x07, 0x41, 0x01], &[0x04, 0x01]]),
                TYP_PART,
            ]),
        ),
        (
            "claims duplicate",
            map(&[ALG, &k, &claims(&[&[0x04, 0x01], &[0x04, 0x02]]), TYP_PART]),
        ),
        (
            "aud is an array",
            map(&[ALG, &k, &claims(&[&[0x03, 0x81, 0x61, b'a']]), TYP_PART]),
        ),
        (
            "aud is empty",
            map(&[ALG, &k, &claims(&[&[0x03, 0x60]]), TYP_PART]),
        ),
        (
            "aud is a bstr",
            map(&[ALG, &k, &claims(&[&[0x03, 0x41, b'a']]), TYP_PART]),
        ),
        ("aud too long", map(&[ALG, &k, &long_aud, TYP_PART])),
        (
            "exp is a float",
            map(&[ALG, &k, &claims(&[&[0x04, 0xf9, 0x3c, 0x00]]), TYP_PART]),
        ),
        (
            "exp is negative",
            map(&[ALG, &k, &claims(&[&[0x04, 0x20]]), TYP_PART]),
        ),
        (
            "exp not minimal",
            map(&[ALG, &k, &claims(&[&[0x04, 0x18, 0x01]]), TYP_PART]),
        ),
        (
            "exp is text",
            map(&[ALG, &k, &claims(&[&[0x04, 0x61, b'1']]), TYP_PART]),
        ),
        (
            "iat is tagged",
            map(&[ALG, &k, &claims(&[&[0x06, 0xc1, 0x01]]), TYP_PART]),
        ),
        (
            "cti is empty",
            map(&[ALG, &k, &claims(&[&[0x07, 0x40]]), TYP_PART]),
        ),
        (
            "cti is text",
            map(&[ALG, &k, &claims(&[&[0x07, 0x61, b'a']]), TYP_PART]),
        ),
        ("cti too long", map(&[ALG, &k, &long_cti, TYP_PART])),
        (
            "alg -8 (deprecated EdDSA)",
            map(&[&[0x01, 0x27], &k, TYP_PART]),
        ),
        (
            "alg -7 (deprecated ES256)",
            map(&[&[0x01, 0x26], &k, TYP_PART]),
        ),
        ("alg 99", map(&[&[0x01, 0x18, 0x63], &k, TYP_PART])),
        ("alg is text", map(&[&[0x01, 0x61, b'x'], &k, TYP_PART])),
        ("alg not minimal", map(&[&[0x01, 0x38, 0x12], &k, TYP_PART])),
        (
            "kid is 7 bytes",
            map(&[ALG, &[0x04, 0x47, 1, 2, 3, 4, 5, 6, 7], TYP_PART]),
        ),
        (
            "kid is 9 bytes",
            map(&[ALG, &[0x04, 0x49, 1, 2, 3, 4, 5, 6, 7, 8, 9], TYP_PART]),
        ),
        (
            "kid is text",
            map(&[
                ALG,
                &[0x04, 0x68, b'k', b'k', b'k', b'k', b'k', b'k', b'k', b'k'],
                TYP_PART,
            ]),
        ),
        ("header is an array", vec![0x83, 0x01, 0x02, 0x03]),
        (
            "header has trailing byte",
            [map(&[ALG, &k, TYP_PART]), vec![0x00]].concat(),
        ),
        (
            "header map indefinite",
            [
                vec![0xbf],
                ALG.to_vec(),
                k.clone(),
                TYP_PART.to_vec(),
                vec![0xff],
            ]
            .concat(),
        ),
    ];
    for (name, protected) in cases {
        let result = open(&signed(&protected)).await;
        assert!(
            matches!(result, Err(ClaimsError::Malformed)),
            "{name}: {result:?}"
        );
    }
}

#[tokio::test]
async fn a_mac_algorithm_is_refused_even_with_a_valid_tag() {
    let hk = hmac_kid();
    for (alg_id, len) in [(0x04u8, 8usize), (0x05, 32)] {
        let protected = map(&[&[0x01, alg_id], &hk, TYP_PART]);
        // A genuine HMAC tag, in a Sign1 (18) and in a Mac0 (17) message.
        let tag = hmac_sign(&sign1_tbs(&protected, AAD, PAYLOAD), len);
        let sign1 = assemble(TAG_SIGN1, &protected, &[0xa0], PAYLOAD, &tag);
        let result = open(&sign1).await;
        assert!(
            matches!(result, Err(ClaimsError::Malformed)),
            "tag 18: {result:?}"
        );
        let tag = hmac_sign(&mac0_tbs(&protected, AAD, PAYLOAD), len);
        let mac0 = assemble(TAG_MAC0, &protected, &[0xa0], PAYLOAD, &tag);
        let result = open(&mac0).await;
        assert!(
            matches!(result, Err(ClaimsError::Malformed)),
            "tag 17: {result:?}"
        );
    }
}

async fn good() -> Vec<u8> {
    let claims = CwtClaims {
        exp: Some(5),
        ..CwtClaims::default()
    };
    sign_claims(&common::ed25519(), TYP, &claims, PAYLOAD, AAD)
        .await
        .unwrap()
}

#[tokio::test]
async fn malformed_outer_structures_are_refused() {
    let good = good().await;
    open(&good).await.expect("control");
    let l = layout(&good);
    let protected = good[l.protected.clone()].to_vec();
    let sig = good[l.signature.clone()].to_vec();
    let join = |parts: &[&[u8]]| parts.concat();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty input", vec![]),
        ("untagged", good[1..].to_vec()),
        ("tag 17", join(&[&[TAG_MAC0], &good[1..]])),
        ("tag 0 around tag 18", join(&[&[0xc0], &good])),
        ("tag head not minimal", join(&[&[0xd8, 0x12], &good[1..]])),
        ("tag 98 (COSE_Sign)", join(&[&[0xd8, 0x62], &good[1..]])),
        (
            "array of 3",
            join(&[&[0xd2, 0x83], &good[2..l.signature.start - 1]]),
        ),
        (
            "array of 5",
            join(&[&good[..1], &[0x85], &good[2..], &[0x40]]),
        ),
        (
            "array indefinite",
            join(&[&[0xd2, 0x9f], &good[2..], &[0xff]]),
        ),
        (
            "protected empty",
            join(&[&[0xd2, 0x84, 0x40, 0xa0], &bstr(PAYLOAD), &bstr(&sig)]),
        ),
        (
            "protected head not minimal",
            join(&[
                &[0xd2, 0x84, 0x58, u8::try_from(protected.len()).unwrap()],
                &protected,
                &[0xa0],
                &bstr(PAYLOAD),
                &bstr(&sig),
            ]),
        ),
        (
            "unprotected non-empty",
            with_unprotected(&good, &[0xa1, 0x04, 0x41, 0x01]),
        ),
        ("unprotected nil", with_unprotected(&good, &[0xf6])),
        ("unprotected empty array", with_unprotected(&good, &[0x80])),
        ("unprotected bstr", with_unprotected(&good, &[0x40])),
        (
            "detached payload",
            join(&[&[0xd2, 0x84], &bstr(&protected), &[0xa0, 0xf6], &bstr(&sig)]),
        ),
        (
            "payload is text",
            join(&[
                &[0xd2, 0x84],
                &bstr(&protected),
                &[0xa0, 0x67],
                b"payload",
                &bstr(&sig),
            ]),
        ),
        (
            "payload indefinite",
            join(&[
                &[0xd2, 0x84],
                &bstr(&protected),
                &[0xa0, 0x5f],
                &bstr(PAYLOAD),
                &[0xff],
                &bstr(&sig),
            ]),
        ),
        (
            "payload head not minimal",
            join(&[
                &[0xd2, 0x84],
                &bstr(&protected),
                &[0xa0, 0x58, 7],
                PAYLOAD,
                &bstr(&sig),
            ]),
        ),
        (
            "signature is text",
            join(&[&good[..l.signature.start - 2], &[0x78, 64], &sig]),
        ),
        ("trailing byte", join(&[&good, &[0x00]])),
        ("trailing message", join(&[&good, &good])),
        ("truncated signature", good[..good.len() - 1].to_vec()),
        (
            "signature 63 bytes",
            join(&[&good[..l.signature.start - 2], &bstr(&sig[..63])]),
        ),
        (
            "signature 65 bytes",
            join(&[
                &good[..l.signature.start - 2],
                &bstr(&[sig.clone(), vec![0]].concat()),
            ]),
        ),
        ("signature missing", good[..l.signature.start - 2].to_vec()),
    ];
    for (name, message) in cases {
        let result = open(&message).await;
        assert!(result.is_err(), "{name} was accepted: {result:?}");
    }
}

/// Every truncation, and eight other values (flipped bits, neighbours, the
/// extremes) of every byte, of valid objects of both algorithms: refused, never a panic. (The only accepted input is
/// the original.)
#[tokio::test]
async fn no_prefix_and_no_single_byte_change_opens_or_panics() {
    let claims = CwtClaims {
        aud: Some("aud".into()),
        exp: Some(70_000),
        iat: Some(3),
        cti: Some(vec![1, 2, 3]),
    };
    for signer in [
        common::signer(cratestack_cose::CoseAlg::Ed25519),
        common::signer(cratestack_cose::CoseAlg::Esp256),
    ] {
        let good = sign_claims(signer.as_ref(), TYP, &claims, PAYLOAD, AAD)
            .await
            .unwrap();
        let resolver = common::resolver();
        let try_open = |bytes: Vec<u8>| {
            let resolver = resolver.clone();
            async move {
                open_claims(resolver.as_ref(), &bytes, TYP, AAD)
                    .await
                    .is_ok()
            }
        };
        assert!(try_open(good.clone()).await);
        for end in 0..good.len() {
            assert!(!try_open(good[..end].to_vec()).await, "prefix {end}");
        }
        for at in 0..good.len() {
            let byte = good[at];
            let values = [
                0x00,
                0xff,
                byte ^ 0x01,
                byte ^ 0x80,
                byte.wrapping_add(1),
                byte.wrapping_sub(1),
                0x18,
                0x5f,
            ];
            for value in values.into_iter().filter(|&value| value != byte) {
                let mut changed = good.clone();
                changed[at] = value;
                assert!(!try_open(changed).await, "byte {at} = {value:#04x}");
            }
        }
    }
}

#[tokio::test]
async fn hostile_heads_do_not_allocate_or_panic() {
    // A bstr claiming 2^64-1 bytes, and a map claiming 2^32 entries.
    let giant_bstr = [vec![0xd2, 0x84, 0x5b], vec![0xff; 8]].concat();
    let giant_map = [vec![0xd2, 0x84, 0x42, 0xba, 0xff, 0xff, 0xff, 0xff]].concat();
    let giant_array = [vec![0xd2, 0x9b], vec![0xff; 8]].concat();
    for message in [
        giant_bstr,
        giant_map,
        giant_array,
        vec![0xd2],
        vec![0xff; 64],
        vec![0x00; 64],
    ] {
        assert!(open(&message).await.is_err());
    }
}
