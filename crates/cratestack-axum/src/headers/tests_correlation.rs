#![cfg(test)]

use axum::http::{HeaderMap, HeaderValue};

use crate::trusted_proxy::ForwardedHeader;

use super::forwarded::parse_client_ip;
use super::traceparent::parse_traceparent;

fn headers_with(name: &'static str, value: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(name, HeaderValue::from_str(value).unwrap());
    headers
}

#[test]
fn traceparent_absent_returns_none() {
    assert!(parse_traceparent(&HeaderMap::new()).unwrap().is_none());
}

#[test]
fn parses_canonical_traceparent_into_trace_id() {
    let h = headers_with(
        "traceparent",
        "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
    );
    let trace_id = parse_traceparent(&h).unwrap().unwrap();
    assert_eq!(trace_id, "0af7651916cd43dd8448eb211c80319c");
}

#[test]
fn rejects_traceparent_with_wrong_segment_count() {
    let h = headers_with("traceparent", "00-deadbeef");
    let err = parse_traceparent(&h).unwrap_err();
    assert_eq!(err.code(), "BAD_REQUEST");
}

#[test]
fn rejects_traceparent_with_short_trace_id() {
    let h = headers_with("traceparent", "00-deadbeef-b7ad6b7169203331-01");
    let err = parse_traceparent(&h).unwrap_err();
    assert_eq!(err.code(), "BAD_REQUEST");
}

#[test]
fn rejects_all_zero_trace_id() {
    let h = headers_with(
        "traceparent",
        "00-00000000000000000000000000000000-b7ad6b7169203331-01",
    );
    let err = parse_traceparent(&h).unwrap_err();
    assert_eq!(err.code(), "BAD_REQUEST");
}

/// #415 Finding 1: `Forwarded` must NOT take priority over
/// `X-Forwarded-For` — the pre-fix behavior this test used to assert was
/// the confirmed spoofing bypass (a real proxy sets XFF and never touches
/// `Forwarded`, so an unconditionally-preferred `Forwarded` header is, in
/// practice, entirely attacker-authored). The default header selection is
/// [`ForwardedHeader::XForwardedFor`]; see `tests_header_precedence.rs`
/// for the full trust-boundary regression coverage of this fix.
#[test]
fn default_header_selection_is_x_forwarded_for_not_forwarded() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "forwarded",
        HeaderValue::from_static("for=192.0.2.43;proto=https"),
    );
    headers.insert("x-forwarded-for", HeaderValue::from_static("10.0.0.1"));
    assert_eq!(
        parse_client_ip(&headers, 1, ForwardedHeader::XForwardedFor),
        Some("10.0.0.1".to_owned())
    );
}

/// `parse_client_ip` only walks the chain and strips the RFC 7239
/// quoted-string syntax (`for="..."`); bracket/port normalization of the
/// selected hop now happens later, via `parse_hop_ip`, once a hop is
/// actually chosen for recording — see `tests_ip_validation.rs` for that
/// coverage (Finding 2). This test documents the split: the raw walk
/// intentionally keeps the brackets.
#[test]
fn forwarded_header_raw_entry_keeps_brackets_until_parse_hop_ip_normalizes_them() {
    let h = headers_with("forwarded", "for=\"[2001:db8::1]\"");
    assert_eq!(
        parse_client_ip(&h, 1, ForwardedHeader::Forwarded),
        Some("[2001:db8::1]".to_owned())
    );
}
