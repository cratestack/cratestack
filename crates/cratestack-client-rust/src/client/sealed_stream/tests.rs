use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};

use reqwest::StatusCode;

use super::{check_unsigned_stream, reject_sealed_success};
use crate::envelope_error::EnvelopeError;
use crate::error::ClientError;

const SEQ: &str = "application/cbor-seq";

fn headers(content_type: &[&'static str], marker: &[&'static str]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for value in content_type {
        map.append(CONTENT_TYPE, HeaderValue::from_static(value));
    }
    for value in marker {
        map.append("cratestack-frames", HeaderValue::from_static(value));
    }
    map
}

fn refused(map: &HeaderMap) -> bool {
    matches!(
        check_unsigned_stream(map, SEQ),
        Err(EnvelopeError::UnsignedFramesRefused)
    )
}

#[test]
fn a_marked_stream_of_the_asked_type_is_accepted() {
    assert!(check_unsigned_stream(&headers(&[SEQ], &["unsigned"]), SEQ).is_ok());
    // The type is compared without its parameters or its case.
    let map = headers(&["Application/CBOR-Seq; charset=x"], &["unsigned"]);
    assert!(check_unsigned_stream(&map, SEQ).is_ok());
}

#[test]
fn a_missing_marker_is_refused() {
    assert!(refused(&headers(&[SEQ], &[])));
}

#[test]
fn a_marker_with_another_value_is_refused() {
    for value in ["signed", "Unsigned", "unsigned ", ""] {
        assert!(refused(&headers(&[SEQ], &[value])), "{value:?}");
    }
}

#[test]
fn a_repeated_marker_is_refused() {
    assert!(refused(&headers(&[SEQ], &["unsigned", "unsigned"])));
}

#[test]
fn another_content_type_is_refused() {
    assert!(refused(&headers(&["application/cbor"], &["unsigned"])));
    assert!(refused(&headers(&["text/event-stream"], &["unsigned"])));
    assert!(refused(&headers(&[], &["unsigned"])));
    assert!(refused(&headers(&[SEQ, SEQ], &["unsigned"])));
}

#[test]
fn the_sse_type_is_checked_against_what_was_asked() {
    let map = headers(&["text/event-stream"], &["unsigned"]);
    assert!(check_unsigned_stream(&map, "text/event-stream").is_ok());
    assert!(check_unsigned_stream(&map, SEQ).is_err());
}

#[test]
fn a_sealed_success_is_an_invalid_response_and_a_sealed_refusal_is_not() {
    for status in [200, 201, 204] {
        let status = StatusCode::from_u16(status).unwrap();
        assert!(
            matches!(
                reject_sealed_success(status),
                Err(ClientError::InvalidResponse(_))
            ),
            "{status}"
        );
    }
    for status in [401, 403, 426, 500] {
        assert!(reject_sealed_success(StatusCode::from_u16(status).unwrap()).is_ok());
    }
}
