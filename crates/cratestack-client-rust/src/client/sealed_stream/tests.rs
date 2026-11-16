use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};

use super::check_unsigned_stream;
use crate::envelope_error::EnvelopeError;

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
