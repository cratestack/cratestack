//! cratestack#1425: the projection-view readers decode a CBOR byte string
//! (what the server now writes for `Bytes`/`Geography`/`Geometry`) as well
//! as the integer array an older server, or a JSON body, carries. They used
//! to decode into `serde_json::Value`, which has no byte-string case.

use cratestack_codec_cbor::CborCodec;
use cratestack_core::CratestackCodec;
use serde_json::json;

use super::views::decode_view_value;
use crate::runtime::wire::{RuntimeHeader, RuntimeResponseWire};

fn response(content_type: &str, body: Vec<u8>) -> RuntimeResponseWire {
    RuntimeResponseWire {
        status_code: 200,
        headers: vec![RuntimeHeader {
            name: "content-type".to_owned(),
            value: content_type.to_owned(),
        }],
        body,
    }
}

/// `{"data": h'0102ff', "label": "x"}` and the same with an integer array.
const BYTE_STRING_BODY: &str = "a26464617461430102ff656c6162656c6178";
const ARRAY_BODY: &str = "a2646461746183010218ff656c6162656c6178";

fn hex(text: &str) -> Vec<u8> {
    let text: String = text.split_whitespace().collect();
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn a_byte_string_body_decodes_into_the_integer_array_a_view_reads() {
    // `get_view`, `list_view` and `list_view_paged` all go through this.
    let value = decode_view_value(
        &CborCodec,
        &response("application/cbor", hex(BYTE_STRING_BODY)),
    )
    .expect("a byte string must decode");
    assert_eq!(value, json!({ "data": [1, 2, 255], "label": "x" }));
}

#[test]
fn an_integer_array_body_still_decodes_identically() {
    let value = decode_view_value(&CborCodec, &response("application/cbor", hex(ARRAY_BODY)))
        .expect("the legacy array form must keep decoding");
    assert_eq!(value, json!({ "data": [1, 2, 255], "label": "x" }));
}

#[test]
fn a_list_body_decodes_each_element() {
    let body = CborCodec
        .encode(&cratestack_core::rpc::RpcFrameValue::from(
            json!([{ "id": 1 }, { "id": 2 }]),
        ))
        .unwrap();
    let value = decode_view_value(&CborCodec, &response("application/cbor", body)).unwrap();
    assert_eq!(value, json!([{ "id": 1 }, { "id": 2 }]));
}
