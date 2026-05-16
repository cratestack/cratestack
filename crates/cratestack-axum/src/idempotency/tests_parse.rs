//! Header-parsing tests.

#![cfg(test)]

use http::HeaderMap;

use super::parse::parse_idempotency_key;

#[test]
fn parses_present_and_absent_keys() {
    let mut headers = HeaderMap::new();
    assert_eq!(parse_idempotency_key(&headers).unwrap(), None);
    headers.insert("idempotency-key", http::HeaderValue::from_static("abc-123"));
    assert_eq!(
        parse_idempotency_key(&headers).unwrap(),
        Some("abc-123".to_owned())
    );
}

#[test]
fn rejects_empty_key() {
    let mut headers = HeaderMap::new();
    headers.insert("idempotency-key", http::HeaderValue::from_static("   "));
    let err = parse_idempotency_key(&headers).unwrap_err();
    assert_eq!(err.code(), "BAD_REQUEST");
}

#[test]
fn rejects_overlong_key() {
    let value = "a".repeat(256);
    let mut headers = HeaderMap::new();
    headers.insert(
        "idempotency-key",
        http::HeaderValue::from_bytes(value.as_bytes()).unwrap(),
    );
    let err = parse_idempotency_key(&headers).unwrap_err();
    assert_eq!(err.code(), "BAD_REQUEST");
}
