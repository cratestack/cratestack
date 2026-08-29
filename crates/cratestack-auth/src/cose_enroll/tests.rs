use super::{
    build_cose_enroll_response_with_key, challenge_signing_key_from,
    parse_cose_enroll_response_with_key,
};
use crate::error::AuthError;
use crate::id_token::decode_signing_key;
use crate::protocol::EnrollResponse;

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
