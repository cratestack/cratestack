//! The payload-type selector headers: one grammar, no normalisation, so a
//! client and a server cannot disagree about what a type is.

use crate::codec::{
    DEFAULT_PAYLOAD_MEDIA_TYPE, PAYLOAD_ACCEPT_HEADER, PAYLOAD_TYPE_HEADER,
    is_sealable_payload_type, parse_payload_accept, parse_payload_type,
    payload_accept_header_value,
};
use crate::error::CratestackError;

#[test]
fn the_names_and_the_default_are_pinned() {
    assert_eq!(PAYLOAD_TYPE_HEADER, "Cratestack-Payload-Type");
    assert_eq!(PAYLOAD_ACCEPT_HEADER, "Cratestack-Payload-Accept");
    assert_eq!(DEFAULT_PAYLOAD_MEDIA_TYPE, "application/cbor");
}

#[test]
fn a_type_is_a_lowercase_token_pair_and_nothing_else() {
    for good in [
        "application/cbor",
        "application/json",
        "application/x-www-form-urlencoded",
        "text/plain",
        "application/vnd.api+json",
    ] {
        assert_eq!(parse_payload_type(good.as_bytes()).unwrap(), good, "{good}");
    }
    let too_long = format!("application/{}", "a".repeat(116));
    assert_eq!(too_long.len(), 128);
    for bad in [
        "",
        "application",
        "application/",
        "/json",
        "application/json/extra",
        "Application/JSON",
        "application/JSON",
        "application/json; charset=utf-8",
        "application/json;",
        " application/json",
        "application/json ",
        "application/*",
        "*/*",
        "text/*",
        "application/js on",
        "application/json,application/cbor",
        "application/\u{e9}",
        too_long.as_str(),
    ] {
        let error = parse_payload_type(bad.as_bytes()).unwrap_err();
        assert!(matches!(error, CratestackError::BadRequest(_)), "{bad:?}");
    }
    assert!(
        parse_payload_type(b"application/\xff").is_err(),
        "not UTF-8"
    );
    let at_limit = format!("application/{}", "a".repeat(115));
    assert_eq!(at_limit.len(), 127);
    assert!(parse_payload_type(at_limit.as_bytes()).is_ok(), "127 bytes");
}

#[test]
fn an_accept_list_keeps_the_senders_order() {
    assert_eq!(
        parse_payload_accept(b"application/json, application/cbor").unwrap(),
        ["application/json", "application/cbor"]
    );
    assert_eq!(
        parse_payload_accept(b"application/json").unwrap(),
        ["application/json"]
    );
}

#[test]
fn an_accept_list_has_one_spelling() {
    let nine = (0..9)
        .map(|i| format!("application/t{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let eight = (0..8)
        .map(|i| format!("application/t{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    assert_eq!(parse_payload_accept(eight.as_bytes()).unwrap().len(), 8);
    for bad in [
        "",
        ",",
        "application/json,application/cbor",
        "application/json,  application/cbor",
        "application/json ,application/cbor",
        "application/json, ",
        "application/json;q=0.5",
        "application/json, application/cbor;q=0.1",
        "application/*",
        "*/*",
        "application/json, application/json",
        "Application/Json",
        nine.as_str(),
    ] {
        assert!(parse_payload_accept(bad.as_bytes()).is_err(), "{bad:?}");
    }
    assert!(parse_payload_accept(b"application/\xff").is_err());
}

#[test]
fn a_list_is_written_the_way_it_is_read() {
    let value = payload_accept_header_value(&["application/json", "application/cbor"]);
    assert_eq!(value, "application/json, application/cbor");
    assert_eq!(
        parse_payload_accept(value.as_bytes()).unwrap(),
        ["application/json", "application/cbor"]
    );
}

#[test]
fn envelopes_streams_and_multiparts_are_never_sealable() {
    for yes in [
        "application/cbor",
        "application/json",
        "application/x-www-form-urlencoded",
        "text/plain",
    ] {
        assert!(is_sealable_payload_type(yes), "{yes}");
    }
    for no in [
        "application/cose",
        "application/cose; cose-type=\"cose-sign1\"",
        "application/cose-key",
        "application/cbor-seq",
        "text/event-stream",
        "multipart/form-data",
        "multipart/mixed",
        "Application/JSON",
        "application/*",
        "",
    ] {
        assert!(!is_sealable_payload_type(no), "{no:?}");
    }
}
