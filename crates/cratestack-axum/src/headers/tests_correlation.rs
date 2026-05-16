#![cfg(test)]

use axum::http::{HeaderMap, HeaderValue};

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

#[test]
fn rfc7239_forwarded_takes_priority_over_x_forwarded_for() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "forwarded",
        HeaderValue::from_static("for=192.0.2.43;proto=https"),
    );
    headers.insert("x-forwarded-for", HeaderValue::from_static("10.0.0.1"));
    assert_eq!(parse_client_ip(&headers), Some("192.0.2.43".to_owned()));
}

#[test]
fn x_forwarded_for_takes_leftmost_address() {
    let h = headers_with("x-forwarded-for", "192.0.2.43, 10.0.0.1");
    assert_eq!(parse_client_ip(&h), Some("192.0.2.43".to_owned()));
}

#[test]
fn client_ip_strips_brackets_around_ipv6() {
    let h = headers_with("forwarded", "for=\"[2001:db8::1]\"");
    assert_eq!(parse_client_ip(&h), Some("2001:db8::1".to_owned()));
}
