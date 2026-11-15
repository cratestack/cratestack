//! Pins `Accept` negotiation exactly as it behaved before the binding was
//! factored out (the table was captured from the old
//! `validate_subscribe_accept_header`), so PR 2 of cratestack#1001 widens it
//! on purpose or not at all.

use axum::http::{HeaderValue, StatusCode};

use super::*;

const NEED: &str = "subscription endpoint requires Accept: text/event-stream";

fn negotiate(accept: Option<&[u8]>) -> Result<SubscribeBinding, CratestackError> {
    let mut headers = HeaderMap::new();
    if let Some(value) = accept {
        headers.insert(header::ACCEPT, HeaderValue::from_bytes(value).unwrap());
    }
    negotiate_subscribe_binding(&headers)
}

#[test]
fn accept_negotiation_table_is_unchanged() {
    let accepted: &[&[u8]] = &[
        b"text/event-stream",
        b"*/*",
        b"application/json, text/event-stream",
        b"application/json,*/*",
        b" text/event-stream ",
    ];
    for accept in accepted {
        assert_eq!(
            negotiate(Some(accept)).unwrap(),
            SubscribeBinding::Sse,
            "{accept:?}"
        );
    }

    let refused: &[&[u8]] = &[
        b"text/*",
        b"application/json",
        b"text/event-stream;q=0",
        b"text/event-stream; q=0.5",
        b"text/event-stream;charset=utf-8",
        b"*/*;q=0",
        b"*/*;q=0.1",
        b"TEXT/EVENT-STREAM",
        b"",
    ];
    for accept in refused {
        let error = negotiate(Some(accept)).unwrap_err();
        let got = String::from_utf8_lossy(accept);
        assert!(matches!(error, CratestackError::NotAcceptable(_)), "{got}");
        assert_eq!(error.status_code(), StatusCode::NOT_ACCEPTABLE);
        assert_eq!(
            error.to_string(),
            format!("not acceptable: {NEED}, got {got}"),
        );
    }
}

#[test]
fn missing_accept_is_406_without_a_got_clause() {
    let error = negotiate(None).unwrap_err();
    assert!(matches!(error, CratestackError::NotAcceptable(_)));
    assert_eq!(error.to_string(), format!("not acceptable: {NEED}"));
}

#[test]
fn non_utf8_accept_is_400() {
    let error = negotiate(Some(b"\xff\xfe")).unwrap_err();
    assert!(matches!(error, CratestackError::BadRequest(_)));
    assert_eq!(
        error.to_string(),
        "bad request: invalid Accept header: failed to convert header to a str",
    );
}
