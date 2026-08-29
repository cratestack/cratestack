use super::{parse_bearer_token, parse_signature_header, uses_signature_scheme};
use crate::error::AuthError;

#[test]
fn recognizes_signature_prefix() {
    assert!(uses_signature_scheme("Signature keyId=\"vk_123\""));
}

#[test]
fn parses_signature_header() {
    let header = parse_signature_header(
        "Signature keyId=\"vk_123\", timestamp=\"2026-04-24T12:00:00Z\", nonce=\"n_123\", signature=\"sig_123\"",
    )
    .expect("signature header should parse");

    assert_eq!(header.key_id, "vk_123");
    assert_eq!(header.nonce, "n_123");
}

#[test]
fn rejects_unquoted_signature_values() {
    let error = parse_signature_header(
        "Signature keyId=vk_123, timestamp=\"2026-04-24T12:00:00Z\", nonce=\"n_123\", signature=\"sig_123\"",
    )
    .expect_err("unquoted values should be rejected");

    assert!(matches!(error, AuthError::MalformedSignatureHeader));
}

#[test]
fn parses_bearer_tokens() {
    assert_eq!(parse_bearer_token("Bearer token-123").unwrap(), "token-123");
}
