use axum::http::{HeaderValue, header};
use cratestack_codec_cbor::CborCodec;
use cratestack_core::CratestackCodec;

use super::*;

fn headers(content_type: Option<&str>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Some(value) = content_type {
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_str(value).unwrap());
    }
    headers
}

#[test]
fn a_json_body_is_the_input_with_or_without_a_content_type() {
    for content_type in [None, Some("application/json"), Some("text/plain")] {
        let input = decode_subscribe_open_body(&headers(content_type), br#"{"roomId":"r1"}"#)
            .expect("json body");
        assert_eq!(
            input,
            serde_json::json!({"roomId": "r1"}),
            "{content_type:?}"
        );
    }
}

#[test]
fn a_cbor_body_is_decoded_by_content_type() {
    let body = CborCodec
        .encode(&serde_json::json!({"roomId": "r1", "limit": 3}))
        .unwrap();
    for content_type in ["application/cbor", "application/cbor; charset=binary"] {
        let input =
            decode_subscribe_open_body(&headers(Some(content_type)), &body).expect("cbor body");
        assert_eq!(input, serde_json::json!({"roomId": "r1", "limit": 3}));
    }
}

#[test]
fn a_cbor_byte_string_reads_as_the_json_array_of_a_bytes_value() {
    let body = CborCodec
        .encode(&RpcFrameValue::Object(
            [("id".to_owned(), RpcFrameValue::Bytes(vec![1, 2]))].into(),
        ))
        .unwrap();
    let input = decode_subscribe_open_body(&headers(Some("application/cbor")), &body).unwrap();
    assert_eq!(input, serde_json::json!({"id": [1, 2]}));
}

#[test]
fn an_empty_body_is_null_and_a_malformed_one_is_a_400() {
    for content_type in [None, Some("application/cbor")] {
        assert_eq!(
            decode_subscribe_open_body(&headers(content_type), b"").unwrap(),
            serde_json::Value::Null,
        );
    }
    let json = decode_subscribe_open_body(&headers(None), b"{").unwrap_err();
    assert!(matches!(json, CratestackError::BadRequest(m) if m.contains("JSON input")));
    // A JSON body sent as CBOR is refused, not guessed at.
    let cbor =
        decode_subscribe_open_body(&headers(Some("application/cbor")), br#"{"a":1}"#).unwrap_err();
    assert!(matches!(cbor, CratestackError::BadRequest(m) if m.contains("CBOR input")));
}
