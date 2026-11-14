//! The keystore callback signer (`esp256_with`, the generic core behind the
//! bridged `esp256_external`) against the shared ESP256 vectors, every
//! outcome kind, and concurrent seals (cratestack#1151).
//!
//! The "keystore" is the crate's RFC 6979 `P256Signer` on the vector key,
//! which is what made the vector bytes, so a callback that signs with it must
//! reproduce them byte for byte whichever way it spells the signature.

#![cfg(feature = "cose")]

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use cratestack_client_flutter::cose::{
    FlutterCallBinding, FlutterClientEnvelope, FlutterCoseAlg, FlutterCoseErrorKind,
    FlutterSealOptions, FlutterServerKey, FlutterSignOutcome, FlutterSignOutcomeKind,
};
use cratestack_cose::{CoseSigner, P256Signer};
use serde_json::Value;

/// The group order, big-endian, for `n − s`.
const N: [u8; 32] = [
    0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0xbc, 0xe6, 0xfa, 0xad, 0xa7, 0x17, 0x9e, 0x84, 0xf3, 0xb9, 0xca, 0xc2, 0xfc, 0x63, 0x25, 0x51,
];

fn vectors(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../cratestack-cose/tests/vectors")
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(path).expect("vector file")).expect("json")
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex"))
        .collect()
}

fn text(value: &Value, field: &str) -> String {
    value[field].as_str().expect(field).to_owned()
}

fn call(binding: &Value) -> FlutterCallBinding {
    let opt = |value: &Value| value.as_str().map(str::to_owned);
    FlutterCallBinding {
        method: text(binding, "method"),
        route: text(binding, "route"),
        path_params: binding["path_params"]
            .as_array()
            .expect("path_params")
            .iter()
            .map(|value| value.as_str().expect("tstr").to_owned())
            .collect(),
        query: opt(&binding["query"]),
        contract_sha: unhex(binding["contract_sha"].as_str().expect("sha")),
        idempotency_key: opt(&binding["bound_headers"]["idempotency_key"]),
        if_match: opt(&binding["bound_headers"]["if_match"]),
        payload_type: None,
        payload_accept: None,
    }
}

/// How the fake keystore spells a signature.
#[derive(Clone, Copy)]
enum Spelling {
    Raw,
    Der,
    HighS,
    HighSDer,
}

fn der_int(bytes: &[u8]) -> Vec<u8> {
    let trimmed = &bytes[bytes
        .iter()
        .position(|b| *b != 0)
        .unwrap_or(bytes.len() - 1)..];
    let mut out = vec![0x02];
    let pad = trimmed[0] & 0x80 != 0;
    out.push((trimmed.len() + usize::from(pad)) as u8);
    out.extend(pad.then_some(0));
    out.extend_from_slice(trimmed);
    out
}

fn der(raw: &[u8]) -> Vec<u8> {
    let mut body = der_int(&raw[..32]);
    body.extend(der_int(&raw[32..]));
    let mut out = vec![0x30, body.len() as u8];
    out.extend(body);
    out
}

fn high_s(raw: &[u8]) -> Vec<u8> {
    let mut out = raw.to_vec();
    let mut borrow = 0i16;
    for i in (0..32).rev() {
        let diff = i16::from(N[i]) - i16::from(raw[32 + i]) - borrow;
        borrow = i16::from(diff < 0);
        out[32 + i] = diff.rem_euclid(256) as u8;
    }
    out
}

fn spell(raw: Vec<u8>, spelling: Spelling) -> Vec<u8> {
    match spelling {
        Spelling::Raw => raw,
        Spelling::Der => der(&raw),
        Spelling::HighS => high_s(&raw),
        Spelling::HighSDer => der(&high_s(&raw)),
    }
}

struct Key {
    scalar: [u8; 32],
    public: Vec<u8>,
    kid: Vec<u8>,
}

fn key() -> Key {
    let keys = vectors("keys.json");
    Key {
        scalar: unhex(keys["p256"]["scalar"].as_str().unwrap())
            .try_into()
            .unwrap(),
        public: unhex(&text(&keys["p256"], "public_sec1_uncompressed")),
        kid: unhex(&text(&keys["p256"], "kid")),
    }
}

fn server_keys() -> Vec<FlutterServerKey> {
    vec![FlutterServerKey {
        alg: FlutterCoseAlg::Esp256,
        bytes: key().public,
    }]
}

fn pinned(iat: i64, cti: &str) -> Option<FlutterSealOptions> {
    Some(FlutterSealOptions {
        fixed_iat: Some(iat),
        fixed_cti: Some(unhex(cti)),
    })
}

type CoseResult<T> = Result<T, cratestack_client_flutter::cose::FlutterCoseError>;

/// An envelope over `public` whose keystore waits `delay`, signs the bytes it
/// is given with the vector key (RFC 6979, raw), and answers `answer(raw)`.
fn envelope<A>(
    public: Vec<u8>,
    options: Option<FlutterSealOptions>,
    delay: Duration,
    answer: A,
) -> CoseResult<FlutterClientEnvelope>
where
    A: Fn(Vec<u8>) -> FlutterSignOutcome + Send + Sync + 'static,
{
    let signer = Arc::new(P256Signer::from_scalar(&key().scalar).expect("scalar"));
    let answer = Arc::new(answer);
    FlutterClientEnvelope::esp256_with(
        public,
        move |tbs| {
            let (signer, answer) = (signer.clone(), answer.clone());
            async move {
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
                answer(signer.sign(&tbs).await.expect("keystore signs"))
            }
        },
        server_keys(),
        "payments".to_owned(),
        options,
    )
}

fn signing(spelling: Spelling) -> impl Fn(Vec<u8>) -> FlutterSignOutcome + Send + Sync + 'static {
    move |raw| FlutterSignOutcome::signed(spell(raw, spelling))
}

fn outcome(kind: FlutterSignOutcomeKind, message: &str) -> FlutterSignOutcome {
    FlutterSignOutcome::of(kind, message)
}

/// A request `case` of the vectors: its envelope pinned to `iat`/`cti`.
fn pinned_case(case: &Value) -> Option<FlutterSealOptions> {
    pinned(
        i64::try_from(case["iat"].as_u64().unwrap()).unwrap(),
        case["cti"].as_str().unwrap(),
    )
}

async fn seal(envelope: &FlutterClientEnvelope, case: &Value) -> CoseResult<Vec<u8>> {
    envelope
        .seal_request(
            unhex(case["payload"].as_str().unwrap()),
            call(&case["binding"]),
        )
        .await
}

fn esp256_requests() -> Vec<Value> {
    vectors("unary.json")["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["direction"] == "request" && case["key"] == "p256")
        .cloned()
        .collect()
}

#[tokio::test]
async fn the_four_esp256_request_vectors_seal_byte_for_byte_in_every_spelling() {
    let cases = esp256_requests();
    assert_eq!(cases.len(), 4);
    for case in &cases {
        for spelling in [
            Spelling::Raw,
            Spelling::Der,
            Spelling::HighS,
            Spelling::HighSDer,
        ] {
            let name = text(case, "name");
            let envelope = envelope(
                key().public,
                pinned_case(case),
                Duration::ZERO,
                signing(spelling),
            )
            .expect("envelope");
            assert_eq!(envelope.kid(), key().kid, "{name}");
            let got = seal(&envelope, case)
                .await
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(got, unhex(case["cose"].as_str().unwrap()), "{name}");
        }
    }
}

#[tokio::test]
async fn an_spki_public_key_is_the_same_signer_as_its_sec1_point() {
    let mut spki = unhex("3059301306072a8648ce3d020106082a8648ce3d030107034200");
    spki.extend_from_slice(&key().public);
    let case = &esp256_requests()[0];
    let envelope = envelope(
        spki,
        pinned_case(case),
        Duration::ZERO,
        signing(Spelling::Der),
    )
    .expect("spki");
    assert_eq!(envelope.kid(), key().kid);
    let got = seal(&envelope, case).await.expect("seal");
    assert_eq!(got, unhex(case["cose"].as_str().unwrap()));
}

#[tokio::test]
async fn an_invalid_public_key_is_misuse_at_creation() {
    let mut off_curve = key().public;
    off_curve[64] ^= 1;
    for bad in [off_curve, vec![4; 65], vec![0x30, 0x01, 0x00], Vec::new()] {
        let error = envelope(bad, None, Duration::ZERO, signing(Spelling::Raw))
            .err()
            .expect("refused");
        assert_eq!(error.kind, FlutterCoseErrorKind::Misuse);
    }
}

#[tokio::test]
async fn each_non_signature_outcome_is_its_own_error_kind() {
    let case = &esp256_requests()[0];
    let cases = [
        (
            FlutterSignOutcomeKind::Cancelled,
            "ignored",
            FlutterCoseErrorKind::SignerCancelled,
            "",
        ),
        (
            FlutterSignOutcomeKind::TimedOut,
            "ignored",
            FlutterCoseErrorKind::SignerTimedOut,
            "",
        ),
        (
            FlutterSignOutcomeKind::Failed,
            "keystore locked",
            FlutterCoseErrorKind::SignerFailed,
            "keystore locked",
        ),
    ];
    for (kind, message, want, want_message) in cases {
        let envelope = envelope(key().public, pinned_case(case), Duration::ZERO, move |_| {
            outcome(kind, message)
        })
        .expect("envelope");
        let error = seal(&envelope, case).await.expect_err("no signature");
        assert_eq!(error.kind, want);
        assert_eq!(error.message, want_message);
    }
}

#[tokio::test]
async fn a_signature_that_is_not_one_is_misuse_not_a_signer_kind() {
    let case = &esp256_requests()[0];
    let wrong_key = |raw: Vec<u8>| {
        // A valid signature by the vector key, over other bytes: flip a bit
        // of `r`, which no longer verifies.
        let mut raw = raw;
        raw[0] ^= 1;
        FlutterSignOutcome::signed(raw)
    };
    let garbage = |_: Vec<u8>| FlutterSignOutcome::signed(vec![1, 2, 3]);
    let empty = |_: Vec<u8>| FlutterSignOutcome::signed(Vec::new());
    let behaviours: [Box<dyn Fn(Vec<u8>) -> FlutterSignOutcome + Send + Sync>; 3] =
        [Box::new(wrong_key), Box::new(garbage), Box::new(empty)];
    for behave in behaviours {
        let envelope =
            envelope(key().public, pinned_case(case), Duration::ZERO, behave).expect("envelope");
        let error = seal(&envelope, case).await.expect_err("refused");
        assert_eq!(error.kind, FlutterCoseErrorKind::Misuse);
    }
}

#[tokio::test]
async fn the_callback_is_called_exactly_once_per_seal() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let case = &esp256_requests()[0];
    let envelope = envelope(
        key().public,
        pinned_case(case),
        Duration::ZERO,
        move |raw| {
            counted.fetch_add(1, Ordering::SeqCst);
            FlutterSignOutcome::signed(raw)
        },
    )
    .expect("envelope");
    seal(&envelope, case).await.expect("seal");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    seal(&envelope, case).await.expect("seal again");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn concurrent_seals_keep_their_outcomes_apart() {
    let case = &esp256_requests()[0];
    // One envelope whose keystore cancels the first request it is asked
    // about and signs the rest, each after a suspension.
    let asked = Arc::new(AtomicUsize::new(0));
    let envelope = envelope(
        key().public,
        pinned_case(case),
        Duration::from_millis(20),
        move |raw| {
            if asked.fetch_add(1, Ordering::SeqCst) == 0 {
                outcome(FlutterSignOutcomeKind::Cancelled, "")
            } else {
                FlutterSignOutcome::signed(raw)
            }
        },
    )
    .expect("envelope");
    let (a, b, c) = tokio::join!(
        seal(&envelope, case),
        seal(&envelope, case),
        seal(&envelope, case)
    );
    let results = [a, b, c];
    let cancelled = results
        .iter()
        .filter(|r| matches!(r, Err(e) if e.kind == FlutterCoseErrorKind::SignerCancelled))
        .count();
    let signed = results.iter().filter(|r| r.is_ok()).count();
    assert_eq!((cancelled, signed), (1, 2));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn seals_that_hop_threads_between_polls_keep_their_outcomes() {
    let case = Arc::new(esp256_requests()[0].clone());
    // The keystore decides from the bytes it is asked to sign, so each
    // payload has one right answer whatever the arrival order.
    let envelope = Arc::new(
        envelope(
            key().public,
            pinned_case(&case),
            Duration::from_millis(5),
            |raw| {
                if raw.iter().map(|b| usize::from(*b)).sum::<usize>() % 2 == 0 {
                    outcome(FlutterSignOutcomeKind::Cancelled, "")
                } else {
                    FlutterSignOutcome::signed(raw)
                }
            },
        )
        .expect("envelope"),
    );
    let seal_payload = |envelope: Arc<FlutterClientEnvelope>, case: Arc<Value>, n: u8| async move {
        envelope
            .seal_request(vec![n], call(&case["binding"]))
            .await
            .map_err(|e| e.kind)
    };
    let mut expected = Vec::new();
    for n in 0..16u8 {
        expected.push(seal_payload(envelope.clone(), case.clone(), n).await);
    }
    assert!(expected.iter().any(Result::is_ok), "no signed payload");
    assert!(expected.iter().any(Result::is_err), "no cancelled payload");
    let tasks: Vec<_> = (0..16u8)
        .map(|n| tokio::spawn(seal_payload(envelope.clone(), case.clone(), n)))
        .collect();
    for (n, task) in tasks.into_iter().enumerate() {
        let got = task.await.expect("join");
        assert_eq!(got.is_ok(), expected[n].is_ok(), "payload {n}");
        if let (Err(got), Err(want)) = (&got, &expected[n]) {
            assert_eq!(got, want, "payload {n}");
            assert_eq!(*got, FlutterCoseErrorKind::SignerCancelled);
        }
    }
}

#[tokio::test]
async fn a_failure_in_one_seal_does_not_leak_into_the_next() {
    let case = &esp256_requests()[0];
    let first = Arc::new(AtomicUsize::new(0));
    let envelope = envelope(
        key().public,
        pinned_case(case),
        Duration::ZERO,
        move |raw| {
            if first.fetch_add(1, Ordering::SeqCst) == 0 {
                outcome(FlutterSignOutcomeKind::TimedOut, "")
            } else {
                FlutterSignOutcome::signed(raw)
            }
        },
    )
    .expect("envelope");
    let error = seal(&envelope, case).await.expect_err("timed out");
    assert_eq!(error.kind, FlutterCoseErrorKind::SignerTimedOut);
    seal(&envelope, case).await.expect("the next seal signs");
}
