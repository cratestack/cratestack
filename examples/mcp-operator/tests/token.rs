//! The example `AuthProvider`'s checks, one refusal each. No database.
//!
//! Each refusal is a token that is right in every way but one, so a check
//! removed from `TokenVerifier::verify` fails exactly one test here.

use cratestack::Value;
use cratestack::serde_json::json;
use mcp_operator_example::token::{ISSUER, TokenVerifier, mint, mint_claims};

const KEY: &[u8] = b"test-only signing key, at least 32 bytes long";
const AUDIENCE: &str = "http://127.0.0.1:8787/mcp";

fn verifier() -> TokenVerifier {
    TokenVerifier::new(KEY, AUDIENCE).expect("a long enough key")
}

fn claims() -> cratestack::serde_json::Value {
    json!({ "iss": ISSUER, "aud": AUDIENCE, "exp": 4_102_444_800_i64, "id": "u-1", "role": "editor" })
}

fn refused(token: &str) -> String {
    match verifier().verify(token) {
        Ok(_) => panic!("accepted: {token}"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn a_valid_token_becomes_exactly_its_id_and_role() {
    let token = mint(KEY, AUDIENCE, "u-1", "editor", 60);
    let ctx = verifier().verify(&token).expect("valid");
    assert_eq!(ctx.auth_field("id"), Some(&Value::String("u-1".into())));
    assert_eq!(
        ctx.auth_field("role"),
        Some(&Value::String("editor".into()))
    );
}

#[test]
fn a_token_for_another_audience_is_refused() {
    let mut body = claims();
    body["aud"] = json!("cratestack://blog");
    assert!(refused(&mint_claims(KEY, &body)).contains("audience"));
}

/// Exact match, not a prefix: an audience that merely starts with this
/// resource names some other endpoint.
#[test]
fn an_audience_that_only_extends_this_one_is_refused() {
    for aud in [format!("{AUDIENCE}/admin"), format!("{AUDIENCE}.evil.test")] {
        let mut body = claims();
        body["aud"] = json!(aud);
        assert!(refused(&mint_claims(KEY, &body)).contains("audience"));
    }
}

#[test]
fn a_token_from_another_issuer_is_refused() {
    let mut body = claims();
    body["iss"] = json!("https://evil.example.test");
    assert!(refused(&mint_claims(KEY, &body)).contains("issuer"));
}

#[test]
fn an_expired_token_is_refused() {
    let mut body = claims();
    body["exp"] = json!(1);
    assert!(refused(&mint_claims(KEY, &body)).contains("expired"));
}

#[test]
fn a_token_without_an_expiry_is_refused() {
    let mut body = claims();
    body.as_object_mut().unwrap().remove("exp");
    assert!(refused(&mint_claims(KEY, &body)).contains("expired"));
}

#[test]
fn a_token_not_yet_valid_is_refused() {
    let mut body = claims();
    body["nbf"] = json!(4_102_444_000_i64);
    assert!(refused(&mint_claims(KEY, &body)).contains("not yet valid"));
    // ...and one whose `nbf` has passed is accepted.
    body["nbf"] = json!(1);
    verifier().verify(&mint_claims(KEY, &body)).expect("valid");
}

#[test]
fn a_token_signed_with_another_key_is_refused() {
    let token = mint_claims(b"another key that is also 32 bytes long!", &claims());
    assert!(refused(&token).contains("signature"));
}

#[test]
fn a_token_without_a_role_is_refused() {
    let mut body = claims();
    body.as_object_mut().unwrap().remove("role");
    assert!(refused(&mint_claims(KEY, &body)).contains("role"));
}

#[test]
fn claims_other_than_id_and_role_never_reach_the_context() {
    let mut body = claims();
    body["tenant"] = json!("t-1");
    let ctx = verifier().verify(&mint_claims(KEY, &body)).expect("valid");
    assert_eq!(ctx.auth_field("tenant"), None);
}

#[test]
fn a_short_signing_key_is_refused() {
    assert!(TokenVerifier::new(b"secret", AUDIENCE).is_err());
}
