#![cfg(test)]

use axum::http::{HeaderMap, HeaderValue, header};

use super::etag::parse_if_match_version;

fn header_map_with_if_match(value: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::IF_MATCH, HeaderValue::from_str(value).unwrap());
    headers
}

#[test]
fn returns_none_when_header_absent() {
    let headers = HeaderMap::new();
    assert_eq!(parse_if_match_version(&headers).unwrap(), None);
}

#[test]
fn parses_strong_quoted_integer() {
    let headers = header_map_with_if_match("\"42\"");
    assert_eq!(parse_if_match_version(&headers).unwrap(), Some(42));
}

#[test]
fn rejects_unquoted_payload() {
    let headers = header_map_with_if_match("42");
    let error = parse_if_match_version(&headers).unwrap_err();
    assert_eq!(error.code(), "BAD_REQUEST");
}

#[test]
fn rejects_wildcard() {
    let headers = header_map_with_if_match("*");
    let error = parse_if_match_version(&headers).unwrap_err();
    assert_eq!(error.code(), "BAD_REQUEST");
}

#[test]
fn rejects_non_integer_payload() {
    let headers = header_map_with_if_match("\"v42\"");
    let error = parse_if_match_version(&headers).unwrap_err();
    assert_eq!(error.code(), "BAD_REQUEST");
}
