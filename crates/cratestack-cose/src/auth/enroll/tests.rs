use super::{
    build_cose_enroll_response_with_key, challenge_signing_key_from,
    parse_cose_enroll_response_with_key,
};
use cratestack_auth::{AuthError, EnrollResponse, decode_signing_key};

/// A key used ONLY in tests. Never reuse this (or any other committed
/// literal) as a real challenge-signing key — see the doc comment on
/// `challenge_signing_key` for why hardcoded seeds are unsafe here.
/// Freshly generated for this absorption, distinct from any key that
/// has ever appeared in this workspace's git history.
fn test_challenge_signing_key_b64() -> &'static str {
    "HFvcdcj5MyyDnkwLJIVptLdkOefDJ_xQXHby1rIIbFc"
}

/// COSE build/parse logic, tested in isolation from env-var loading —
/// no process environment is touched (this workspace forbids
/// `unsafe_code`, and `std::env::set_var` requires `unsafe` as of the
/// 2024 edition). Env-var loading itself is covered separately by the
/// two `challenge_signing_key_from` tests below.
#[test]
fn round_trips_cose_enroll_responses() {
    let key = decode_signing_key(test_challenge_signing_key_b64()).expect("test key decodes");

    let response = EnrollResponse {
        enrollment_id: "enr_123".to_string(),
        key_id: "vk_123".to_string(),
        challenge: "chl_123".to_string(),
        challenge_format: "cose".to_string(),
        expires_at: chrono::Utc::now(),
    };

    let encoded =
        build_cose_enroll_response_with_key(&response, &key).expect("cose build should succeed");
    let decoded =
        parse_cose_enroll_response_with_key(&encoded, &key).expect("cose payload should parse");

    assert_eq!(decoded.enrollment_id, response.enrollment_id);
    assert_eq!(decoded.key_id, response.key_id);
    assert_eq!(decoded.challenge, response.challenge);
    assert_eq!(decoded.challenge_format, "cose");
}

#[test]
fn challenge_signing_key_fails_closed_when_env_var_is_absent() {
    let result = challenge_signing_key_from(|_| Err(std::env::VarError::NotPresent));

    match result {
        Ok(_) => panic!("challenge_signing_key must fail closed without a signing key"),
        Err(error) => assert!(matches!(error, AuthError::MissingSigningKeyEnv(_))),
    }
}

#[test]
fn challenge_signing_key_fails_closed_when_env_var_is_whitespace_only() {
    let result = challenge_signing_key_from(|_| Ok("   \n\t  ".to_string()));

    match result {
        Ok(_) => panic!("challenge_signing_key must fail closed on a whitespace-only value"),
        Err(error) => assert!(matches!(error, AuthError::MissingSigningKeyEnv(_))),
    }
}

/// The exact bytes `build_cose_enroll_response_with_key` produces for a
/// fixed response and the fixed test key. Pinned in `cratestack-auth`
/// before the enrolment code moved here (cratestack#1005 part B) and not
/// changed since, so the move is provably byte-neutral: Ed25519 is
/// deterministic, so any
/// change to the payload encoding, the protected header (the legacy 35-byte
/// `kid`, alg `-8`), the empty external AAD or the tagging shows up here.
const GOLDEN_ENROLL_RESPONSE_HEX: &str = concat!(
    "d2845829a201270458236372617465737461636b2d617574682d656e726f6c6c2d6368616c6c656e",
    "67652d7631a0587da56c656e726f6c6c6d656e7449646a656e725f676f6c64656e656b6579496469",
    "766b5f676f6c64656e696368616c6c656e67656a63686c5f676f6c64656e6f6368616c6c656e6765",
    "466f726d617464636f736569657870697265734174781e323032362d30392d32315431343a31333a",
    "32302e3132333435363738395a58407c5159e9b220688b8759f7ae1ed7dd56335ae3ad5e0937f03f",
    "0189bf499060b1e898e78132862eaf70c7d0f60d6f5b3a9ea5f17c9ee595dcdb135896bb7f7f09",
);

fn golden_enroll_response() -> EnrollResponse {
    EnrollResponse {
        enrollment_id: "enr_golden".to_string(),
        key_id: "vk_golden".to_string(),
        challenge: "chl_golden".to_string(),
        challenge_format: "cose".to_string(),
        // 2026-09-21T14:13:20.123456789Z: sub-second digits included so a
        // change in how the timestamp is rendered is caught too.
        expires_at: chrono::DateTime::from_timestamp(1_790_000_000, 123_456_789).expect("in range"),
    }
}

#[test]
fn cose_enroll_response_matches_the_golden_bytes() {
    let key = decode_signing_key(test_challenge_signing_key_b64()).expect("test key decodes");
    let encoded = build_cose_enroll_response_with_key(&golden_enroll_response(), &key)
        .expect("cose build should succeed");
    let hex: String = encoded.iter().map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(hex, GOLDEN_ENROLL_RESPONSE_HEX);

    let decoded = parse_cose_enroll_response_with_key(&encoded, &key).expect("golden parses");
    assert_eq!(decoded.expires_at, golden_enroll_response().expires_at);
    assert_eq!(decoded.enrollment_id, "enr_golden");
}
