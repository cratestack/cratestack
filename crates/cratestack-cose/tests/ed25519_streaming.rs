//! Ed25519 is signed and verified over the to-be-signed structure in
//! pieces (maintainer decision on cratestack#1005, 2026-09-25). Two things
//! have to hold for that to be safe, and both are checked against
//! `ed25519-dalek`'s contiguous APIs, not against the crate itself:
//!
//! 1. **Signing is unchanged**: the streamed signature is byte for byte
//!    `SigningKey::sign` over the contiguous `Sig_structure` (built here by
//!    `coset`), at every payload-head size. (`tests/vectors.rs` checks the
//!    same for every Ed25519 vector, and the request vectors are the bytes
//!    the contiguous signer produced before.)
//! 2. **Verifying is exactly as strict as `verify_strict`**: the streamed
//!    path accepts a message if and only if `verify_strict` accepts its
//!    contiguous structure, including for the forgeries only the strict
//!    checks stop (a weak key, a small-order `R`, the `S + L` twin), and
//!    for the mixed-order keys and `R` it does not stop (so it is not
//!    stricter either). The one deliberate exception is the `S + L` twin
//!    when something in the build enables `ed25519-dalek`'s
//!    `legacy_compatibility`: `verify_strict` then accepts it, and the
//!    envelope still refuses it (`src/keys/verify.rs`).

mod common;

use std::ops::Range;
use std::sync::Arc;

use bytes::Bytes;
use common::backends::FixedResolver;
use common::forge::{self, TAG_SIGN1};
use common::{CTI_16, IAT, rpc_request};
use cratestack_core::{CratestackError, InMemoryNonceStore};
use cratestack_cose::{CoseAlg, CoseSigner, CoseVerifyKey, external_aad};
use curve25519_dalek::constants::EIGHT_TORSION;
use curve25519_dalek::edwards::EdwardsPoint;
use curve25519_dalek::scalar::Scalar;
use curve25519_dalek::traits::Identity as _;
use ed25519_dalek::Signer as _;
use sha2::{Digest, Sha512};

/// The payload sizes around every `bstr` head-length boundary.
const SIZES: [usize; 9] = [0, 1, 23, 24, 255, 256, 65_535, 65_536, 200_000];

/// `(protected, payload, signature)` of a Sign1 message, read with heads
/// of any length (the shared `forge::layout` stops at 2-byte heads).
fn split(message: &[u8]) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    fn bstr(bytes: &[u8], at: &mut usize) -> Vec<u8> {
        let initial = bytes[*at];
        assert_eq!(initial >> 5, 2, "a bstr at {at}");
        let (len, head) = match initial & 0x1f {
            n @ 0..=23 => (usize::from(n), 1),
            24 => (usize::from(bytes[*at + 1]), 2),
            25 => (
                usize::from(u16::from_be_bytes([bytes[*at + 1], bytes[*at + 2]])),
                3,
            ),
            26 => {
                let len: [u8; 4] = bytes[*at + 1..*at + 5].try_into().expect("4");
                (usize::try_from(u32::from_be_bytes(len)).expect("fits"), 5)
            }
            other => panic!("head {other}"),
        };
        let out = bytes[*at + head..*at + head + len].to_vec();
        *at += head + len;
        out
    }
    assert_eq!(&message[..2], &[TAG_SIGN1, 0x84]);
    let mut at = 2;
    let protected = bstr(message, &mut at);
    assert_eq!(message[at], 0xa0);
    at += 1;
    let payload = bstr(message, &mut at);
    let signature = bstr(message, &mut at);
    assert_eq!(at, message.len());
    (protected, payload, signature)
}

#[tokio::test]
async fn streamed_signatures_equal_contiguous_signing_at_every_head_size() {
    let client = common::client(CoseAlg::Ed25519, IAT, CTI_16);
    let server = common::server(CoseAlg::Ed25519, IAT);
    let request = rpc_request();
    for size in SIZES {
        let payload: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
        let sealed_request = client.seal_request(&payload, &request).await.expect("seal");
        let response = common::response_to(&request, &sealed_request, 200);
        let sealed_response = server
            .seal_response(&payload, &response)
            .await
            .expect("seal");
        for (message, bind) in [(&sealed_request, &request), (&sealed_response, &response)] {
            let (protected, sealed_payload, signature) = split(message);
            assert_eq!(sealed_payload, payload, "{size}");
            let tbs = forge::sign1_tbs(&protected, &external_aad(bind).expect("aad"), &payload);
            assert_eq!(
                signature,
                forge::ed25519_sign(&tbs),
                "{size}-byte payload: streamed signature differs from SigningKey::sign"
            );
        }
    }
}

/// `sign_chunks` over any split of the bytes (empty pieces included) is
/// `sign` over their concatenation, and `SigningKey::sign`.
#[tokio::test]
async fn sign_chunks_is_independent_of_the_split() {
    let signer = common::ed25519();
    let message: Vec<u8> = (0..1000_u32).map(|i| (i * 7) as u8).collect();
    let expected = forge::ed25519_sign(&message);
    assert_eq!(signer.sign(&message).await.expect("sign"), expected);
    for cut in [0, 1, 63, 64, 500, 999, 1000] {
        let (head, tail) = message.split_at(cut);
        for chunks in [
            vec![head, tail],
            vec![&[][..], head, &[], tail, &[]],
            message.chunks(cut.max(1)).collect(),
        ] {
            let signed = signer
                .sign_chunks(&chunks)
                .expect("an in-process signer streams")
                .expect("sign");
            assert_eq!(signed, expected, "cut at {cut}");
        }
    }
}

/// Open `(protected, payload, signature)` as a request with `key` as the
/// only candidate, and say whether it verified.
async fn opens(key: &CoseVerifyKey, protected: &[u8], payload: &[u8], signature: &[u8]) -> bool {
    let body = forge::assemble(TAG_SIGN1, protected, &[0xa0], payload, signature);
    let server = common::server_with(
        CoseAlg::Ed25519,
        IAT,
        Arc::new(FixedResolver(vec![key.clone()])),
        Arc::new(InMemoryNonceStore::new()),
    );
    match server.open_request(Bytes::from(body), &rpc_request()).await {
        Ok(_) => true,
        Err(CratestackError::Unauthorized(_)) => false,
        Err(other) => panic!("not a verification outcome: {other:?}"),
    }
}

/// `verify_strict` over the contiguous structure: the reference.
fn strict(public: &[u8; 32], tbs: &[u8], signature: &[u8]) -> bool {
    let Ok(key) = ed25519_dalek::VerifyingKey::from_bytes(public) else {
        return false;
    };
    let Ok(signature) = ed25519_dalek::Signature::from_slice(signature) else {
        return false;
    };
    key.verify_strict(tbs, &signature).is_ok()
}

/// A request protected header for `public`'s `kid`, with `cti` ending in
/// `counter` (so a test can search for a header a forgery works with).
fn protected_for(public: &[u8; 32], counter: u16) -> Vec<u8> {
    let key = CoseVerifyKey::ed25519(public).expect("a curve point");
    let mut cti = common::unhex(CTI_16);
    cti[14..].copy_from_slice(&counter.to_be_bytes());
    forge::request_protected(-19, &key.kid(), u32::try_from(IAT).expect("u32"), &cti)
}

/// Whether the plain, cofactorless check (`[S]B = R + [k]A`, what a
/// streamed verify does without the strict checks) accepts.
fn cofactorless(public: &[u8; 32], tbs: &[u8], signature: &[u8; 64]) -> bool {
    use ed25519_dalek::Verifier as _;
    ed25519_dalek::VerifyingKey::from_bytes(public)
        .expect("point")
        .verify(tbs, &ed25519_dalek::Signature::from_bytes(signature))
        .is_ok()
}

/// The challenge `k = SHA-512(R ‖ A ‖ M) mod L` of RFC 8032.
fn challenge(r: &[u8; 32], a: &[u8; 32], message: &[u8]) -> Scalar {
    let digest: [u8; 64] = Sha512::new()
        .chain_update(r)
        .chain_update(a)
        .chain_update(message)
        .finalize()
        .into();
    Scalar::from_bytes_mod_order_wide(&digest)
}

/// A case: a public key and a signature over `tbs`, which the plain
/// cofactorless equation may accept but `verify_strict` may not.
struct Forgery {
    what: &'static str,
    public: [u8; 32],
    signature: [u8; 64],
    /// The protected header the signature covers (its `kid` is the key's).
    protected: Vec<u8>,
}

fn forgeries(payload: &[u8]) -> Vec<Forgery> {
    let aad = external_aad(&rpc_request()).expect("aad");
    let identity = EdwardsPoint::identity().compress().to_bytes();
    let basepoint = EdwardsPoint::mul_base(&Scalar::ONE).compress().to_bytes();
    let signing = ed25519_dalek::SigningKey::from_bytes(&common::ED25519_SEED);
    let honest = signing.verifying_key().to_bytes();
    let tbs = |protected: &[u8]| forge::sign1_tbs(protected, &aad, payload);
    let mut out = Vec::new();
    // Each of the eight small-order ("weak") keys, with R = identity and
    // S = 0: [0]B = identity + [k]A holds whenever the order of A divides
    // k, for about one message in eight at worst (always for the identity).
    // Search the `cti` for a header where it holds.
    for torsion in EIGHT_TORSION {
        let public = torsion.compress().to_bytes();
        let mut signature = [0; 64];
        signature[..32].copy_from_slice(&identity);
        let protected = (0..1024)
            .map(|counter| protected_for(&public, counter))
            .find(|protected| cofactorless(&public, &tbs(protected), &signature))
            .expect("a message the weak key forgery works for");
        out.push(Forgery {
            what: "weak key, R = identity, S = 0",
            public,
            signature,
            protected,
        });
    }
    // The identity key with an ordinary R: [1]B = B + [k]·identity. Only
    // the small-order-A check refuses it.
    let mut signature = [0; 64];
    signature[..32].copy_from_slice(&basepoint);
    signature[32..].copy_from_slice(Scalar::ONE.as_bytes());
    out.push(Forgery {
        what: "identity key, R = B, S = 1",
        public: identity,
        signature,
        protected: protected_for(&identity, 0),
    });
    // The honest key with R = identity (small order) and S = k·a:
    // [k·a]B = identity + [k]A holds. Only the small-order-R check refuses
    // it. Building it needs the secret scalar, which the test key's is.
    let protected = protected_for(&honest, 0);
    let s = challenge(&identity, &honest, &tbs(&protected)) * signing.to_scalar();
    let mut signature = [0; 64];
    signature[..32].copy_from_slice(&identity);
    signature[32..].copy_from_slice(s.as_bytes());
    out.push(Forgery {
        what: "honest key, R = identity, S = k·a",
        public: honest,
        signature,
        protected,
    });
    out
}

/// Each forgery passes the plain cofactorless check, so a streamed verify
/// without the strict checks would accept it; `verify_strict` and the
/// envelope both refuse it.
#[tokio::test]
async fn streamed_verify_refuses_what_verify_strict_refuses() {
    let payload = common::fixture::payment_bytes();
    let aad = external_aad(&rpc_request()).expect("aad");
    let forgeries = forgeries(&payload);
    assert_eq!(forgeries.len(), 10);
    for forgery in forgeries {
        let tbs = forge::sign1_tbs(&forgery.protected, &aad, &payload);
        let key = CoseVerifyKey::ed25519(&forgery.public).expect("a curve point");
        assert!(
            cofactorless(&forgery.public, &tbs, &forgery.signature),
            "{}: not a forgery",
            forgery.what
        );
        assert!(
            !strict(&forgery.public, &tbs, &forgery.signature),
            "{}: strict",
            forgery.what
        );
        assert!(
            !opens(&key, &forgery.protected, &payload, &forgery.signature).await,
            "{}: the streamed verify accepted what verify_strict refuses",
            forgery.what
        );
    }
}

/// The `S + L` twin of an honest signature satisfies the verification
/// equation (`[S + L]B = [S]B`), so only a canonical-`S` check refuses it.
/// `verify_strict` refuses it by default and accepts it once anything in
/// the build enables `ed25519-dalek/legacy_compatibility`, which only
/// checks that `S`'s top three bits are clear; so this test does not use
/// `verify_strict` as its reference, and the envelope must refuse the twin
/// under either feature set (its own `S < L` check, `src/keys/verify.rs`).
#[tokio::test]
async fn the_s_plus_l_twin_is_refused_whatever_ed25519_dalek_features() {
    let payload = common::fixture::payment_bytes();
    let aad = external_aad(&rpc_request()).expect("aad");
    let signing = ed25519_dalek::SigningKey::from_bytes(&common::ED25519_SEED);
    let public = signing.verifying_key().to_bytes();
    let key = CoseVerifyKey::ed25519(&public).expect("point");
    let protected = protected_for(&public, 0);
    let tbs = forge::sign1_tbs(&protected, &aad, &payload);
    let honest = signing.sign(&tbs).to_bytes();
    assert!(opens(&key, &protected, &payload, &honest).await, "control");
    let mut twin = honest;
    let l_minus_one = Scalar::ZERO - Scalar::ONE;
    let mut carry = 1_u16; // S + (L - 1) + 1 = S + L
    for (byte, add) in twin[32..].iter_mut().zip(l_minus_one.as_bytes()) {
        let sum = u16::from(*byte) + u16::from(*add) + carry;
        *byte = sum as u8;
        carry = sum >> 8;
    }
    assert_eq!(carry, 0, "S + L fits in 256 bits");
    let twin_s: [u8; 32] = twin[32..].try_into().expect("32 bytes");
    assert!(
        bool::from(Scalar::from_canonical_bytes(twin_s).is_none()),
        "S + L is not canonical"
    );
    assert_eq!(twin[63] & 0xe0, 0, "and passes the top-three-bits check");
    assert!(
        !opens(&key, &protected, &payload, &twin).await,
        "S + L twin accepted"
    );
}

/// Every single-bit change to the payload, the signature and the protected
/// header's `kid`, `iat` and `cti` bytes of an honest request: the envelope
/// accepts it exactly when `verify_strict` over the contiguous structure
/// does (never, for a real change).
#[tokio::test]
async fn every_bit_flip_is_decided_like_verify_strict() {
    let signing = ed25519_dalek::SigningKey::from_bytes(&common::ED25519_SEED);
    let public = signing.verifying_key().to_bytes();
    let key = CoseVerifyKey::ed25519(&public).expect("point");
    let aad = external_aad(&rpc_request()).expect("aad");
    let payload = common::fixture::payment_bytes();
    let protected = protected_for(&public, 0);
    let signature = signing
        .sign(&forge::sign1_tbs(&protected, &aad, &payload))
        .to_bytes();
    assert!(
        opens(&key, &protected, &payload, &signature).await,
        "control"
    );
    // The protected header's `kid`, `iat` and `cti` bytes; a flip anywhere
    // else in it changes the header's structure, which strict parsing
    // refuses before any signature is checked.
    let header_bytes: Vec<usize> = (5..13).chain(17..21).chain(23..39).collect();
    assert_eq!(protected.len(), 39);
    let mut checked = 0;
    for part in 0..3 {
        let positions: Vec<usize> = match part {
            0 => (0..payload.len()).collect(),
            1 => (0..signature.len()).collect(),
            _ => header_bytes.clone(),
        };
        for at in positions {
            for bit in 0..8 {
                let (mut p, mut s, mut h) = (payload.clone(), signature, protected.clone());
                match part {
                    0 => p[at] ^= 1 << bit,
                    1 => s[at] ^= 1 << bit,
                    _ => h[at] ^= 1 << bit,
                }
                let tbs = forge::sign1_tbs(&h, &aad, &p);
                let reference = strict(&public, &tbs, &s);
                let streamed = opens(&key, &h, &p, &s).await;
                assert_eq!(streamed, reference, "part {part}, byte {at}, bit {bit}");
                assert!(!streamed, "part {part}, byte {at}, bit {bit} accepted");
                checked += 1;
            }
        }
    }
    assert!(checked > 1500, "only {checked} flips");
}

/// `(accepted, refused)` over mixed-order keys `A' = aB + T_i`, one per
/// non-identity torsion point `T_i`, signed with `R = rB + T_j` for each
/// `j` in `r_torsion` (`EIGHT_TORSION[0]` is the identity, so `0..1` is an
/// honest `R`) and `S = r + k'·a`, over `messages` headers each.
/// `[S]B = R + [k']A'` holds exactly when `T_j + [k']T_i` is the identity.
/// Panics on the first case the envelope decides unlike `verify_strict`.
async fn mixed_order(r_torsion: Range<usize>, messages: u16) -> (usize, usize) {
    let signing = ed25519_dalek::SigningKey::from_bytes(&common::ED25519_SEED);
    let a = signing.to_scalar();
    let honest = signing.verifying_key().to_edwards();
    let payload = common::fixture::payment_bytes();
    let aad = external_aad(&rpc_request()).expect("aad");
    let (mut accepted, mut refused) = (0, 0);
    for (i, t_i) in EIGHT_TORSION.iter().enumerate().skip(1) {
        let public = (honest + t_i).compress().to_bytes();
        let key = CoseVerifyKey::ed25519(&public).expect("a curve point");
        for j in r_torsion.clone() {
            for counter in 0..messages {
                let protected = protected_for(&public, counter);
                let tbs = forge::sign1_tbs(&protected, &aad, &payload);
                let seed: [u8; 64] = Sha512::new()
                    .chain_update(counter.to_le_bytes())
                    .chain_update([i as u8, j as u8])
                    .finalize()
                    .into();
                let r = Scalar::from_bytes_mod_order_wide(&seed);
                let big_r = (EdwardsPoint::mul_base(&r) + EIGHT_TORSION[j])
                    .compress()
                    .to_bytes();
                let s = r + challenge(&big_r, &public, &tbs) * a;
                let mut signature = [0; 64];
                signature[..32].copy_from_slice(&big_r);
                signature[32..].copy_from_slice(s.as_bytes());
                let reference = strict(&public, &tbs, &signature);
                let streamed = opens(&key, &protected, &payload, &signature).await;
                assert_eq!(
                    streamed, reference,
                    "A = aB + T{i}, R = rB + T{j}, message {counter}"
                );
                if reference {
                    accepted += 1;
                } else {
                    refused += 1;
                }
            }
        }
    }
    (accepted, refused)
}

/// A mixed-order key with an honest `R = rB`: accepted exactly when
/// `[k']T` vanishes (about one message in two, four or eight, by `T`'s
/// order), as `verify_strict` does; it refuses small-order keys, not a
/// torsion component. A verifier stricter than `verify_strict` (refusing
/// any key that is not torsion-free) passes every refusal test above and
/// fails here.
#[tokio::test]
async fn mixed_order_keys_are_decided_like_verify_strict() {
    let (accepted, refused) = mixed_order(0..1, 24).await;
    assert!(
        accepted > 0,
        "no accepted mixed-order key ({refused} refused): vacuous"
    );
    assert!(refused > 0, "no refused mixed-order case");
}

/// A mixed-order key with a mixed-order `R = rB + T_j`, every pair of
/// non-identity torsion points: accepted exactly when `T_j = -[k']T_i`, as
/// `verify_strict` does (it refuses a small-order `R`, not a torsion
/// component).
#[tokio::test]
async fn mixed_order_keys_and_r_are_decided_like_verify_strict() {
    let (accepted, refused) = mixed_order(1..8, 4).await;
    assert!(
        accepted > 0,
        "no accepted mixed-order R ({refused} refused): vacuous"
    );
    assert!(refused > 0, "no refused mixed-order R");
}
