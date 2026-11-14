//! Frames carry byte strings (cratestack#1360): a CBOR `next` / `input`
//! holding a `Uuid` or `Bytes` decodes, and JSON output is what it was.

use serde::Serialize;

use super::*;

const ID: u128 = 0x0123_4567_89ab_cdef_0123_4567_89ab_cdef;

#[derive(Serialize)]
struct Item {
    id: uuid::Uuid,
    data: crate::Value,
}

fn item() -> Item {
    Item {
        id: uuid::Uuid::from_u128(ID),
        data: crate::Value::Bytes(vec![0, 1, 2, 255]),
    }
}

fn cbor<T: Serialize + ?Sized>(value: &T) -> Vec<u8> {
    let mut out = Vec::new();
    let mut ser = minicbor_serde::Serializer::new(&mut out);
    ser.serialize_unit_as_null(true);
    value.serialize(&mut ser).expect("cbor encode");
    out
}

#[test]
fn a_cbor_next_with_byte_strings_decodes() {
    #[derive(Serialize)]
    struct Wire {
        id: u64,
        next: Item,
    }
    let bytes = cbor(&Wire {
        id: 4,
        next: item(),
    });
    let frame: ServerFrame = minicbor_serde::from_slice(&bytes).expect("a byte-string next");
    let Some(RpcFrameValue::Object(fields)) = &frame.next else {
        panic!("expected an object item, got {:?}", frame.next);
    };
    assert_eq!(
        fields["id"],
        RpcFrameValue::Bytes(ID.to_be_bytes().to_vec())
    );
    assert_eq!(fields["data"], RpcFrameValue::Bytes(vec![0, 1, 2, 255]));
    let again: ServerFrame = minicbor_serde::from_slice(&cbor(&frame)).unwrap();
    assert_eq!(again.next, frame.next, "and survives a re-encode");
}

#[test]
fn a_cbor_request_input_with_byte_strings_classifies() {
    #[derive(Serialize)]
    struct Wire {
        id: u64,
        op: &'static str,
        input: Item,
    }
    let bytes = cbor(&Wire {
        id: 1,
        op: "procedure.x",
        input: item(),
    });
    let frame: ClientFrame = minicbor_serde::from_slice(&bytes).expect("a byte-string input");
    match frame.classify().expect("a request") {
        ClientMessage::Request { id, input, .. } => {
            assert_eq!(id, 1);
            assert!(matches!(input, RpcFrameValue::Object(_)));
        }
        other => panic!("expected a request, got {other:?}"),
    }
}

#[test]
fn json_frames_are_byte_identical_to_the_json_value_ones() {
    let json = serde_json::json!({"b": [1, -2, 3.25, null, true], "a": {"y": "s", "x": 1e300}});
    let frame = ServerFrame::next(7, json.clone());
    assert_eq!(
        serde_json::to_string(&frame).unwrap(),
        format!(r#"{{"id":7,"next":{json}}}"#)
    );
    let parsed: ServerFrame = serde_json::from_str(r#"{"id":7,"next":null}"#).unwrap();
    assert_eq!(parsed.next, Some(RpcFrameValue::Null), "present-but-null");
    let request = ClientFrame::request(2, "model.X.subscribe", json.clone());
    assert_eq!(
        serde_json::to_string(&request).unwrap(),
        format!(r#"{{"id":2,"op":"model.X.subscribe","input":{json}}}"#)
    );
}
