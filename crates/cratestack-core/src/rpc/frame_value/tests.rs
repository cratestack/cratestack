use serde::{Deserialize, Serialize};

use super::RpcFrameValue;
use crate::codec::CratestackCodec;

const CORPUS: &[&str] = &[
    r#"{"b":[1,-2,3.25,18446744073709551615,-9223372036854775808,null,true],"a":{"y":"s\n\"q\"","x":1e300}}"#,
    r#"[{"input":{"z":null,"computedParams":"{\"q\":1}"},"op":"x","id":3}]"#,
    r#"{"zebra":1,"apple":{"mid":[],"beta":{}},"Apple":"é"}"#,
];

fn cbor<T: Serialize + ?Sized>(value: &T) -> Vec<u8> {
    let mut out = Vec::new();
    let mut ser = minicbor_serde::Serializer::new(&mut out);
    ser.serialize_unit_as_null(true);
    value.serialize(&mut ser).expect("cbor encode");
    out
}

#[test]
fn json_output_is_byte_identical_to_serde_json_value() {
    for src in CORPUS {
        let value: serde_json::Value = serde_json::from_str(src).unwrap();
        let frame: RpcFrameValue = serde_json::from_str(src).unwrap();
        assert_eq!(
            serde_json::to_vec(&value).unwrap(),
            serde_json::to_vec(&frame).unwrap(),
            "{src}"
        );
        assert_eq!(RpcFrameValue::from(value.clone()), frame, "{src}");
        assert_eq!(serde_json::Value::from(frame), value, "{src}");
    }
}

#[test]
fn cbor_output_is_byte_identical_to_serde_json_value() {
    for src in CORPUS {
        let value: serde_json::Value = serde_json::from_str(src).unwrap();
        let frame: RpcFrameValue = serde_json::from_str(src).unwrap();
        assert_eq!(cbor(&value), cbor(&frame), "{src}");
        let decoded: RpcFrameValue = minicbor_serde::from_slice(&cbor(&value)).unwrap();
        assert_eq!(decoded, frame, "{src}");
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Blob {
    id: uuid::Uuid,
    data: crate::Value,
}

#[test]
fn cbor_byte_strings_and_uuids_round_trip() {
    let blob = Blob {
        id: uuid::Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef),
        data: crate::Value::Bytes(vec![0, 1, 2, 255]),
    };
    let frame: RpcFrameValue = minicbor_serde::from_slice(&cbor(&blob)).unwrap();
    let RpcFrameValue::Object(fields) = &frame else {
        panic!("expected an object, got {frame:?}");
    };
    assert!(matches!(&fields["id"], RpcFrameValue::Bytes(b) if b.len() == 16));
    assert_eq!(fields["data"], RpcFrameValue::Bytes(vec![0, 1, 2, 255]));
    // Keys are re-ordered (`BTreeMap`), so compare values, not bytes.
    let again: RpcFrameValue = minicbor_serde::from_slice(&cbor(&frame)).unwrap();
    assert_eq!(again, frame);
    let back: Blob = minicbor_serde::from_slice(&cbor(&frame)).unwrap();
    assert_eq!(back, blob);
}

#[test]
fn bytes_become_a_number_array_as_json() {
    let value = serde_json::Value::from(RpcFrameValue::Bytes(vec![1, 2]));
    assert_eq!(value, serde_json::json!([1, 2]));
}

#[derive(Clone)]
struct Json;

impl CratestackCodec for Json {
    const CONTENT_TYPE: &'static str = "application/json";

    fn encode<T: Serialize + ?Sized>(
        &self,
        value: &T,
    ) -> Result<Vec<u8>, crate::error::CratestackError> {
        serde_json::to_vec(value).map_err(|e| crate::error::CratestackError::Codec(e.to_string()))
    }

    fn decode<T: for<'de> Deserialize<'de>>(
        &self,
        bytes: &[u8],
    ) -> Result<T, crate::error::CratestackError> {
        serde_json::from_slice(bytes)
            .map_err(|e| crate::error::CratestackError::Codec(e.to_string()))
    }
}

#[test]
fn encode_with_and_decode_with_round_trip_through_a_codec() {
    let frame = RpcFrameValue::encode_with(&Json, &vec![1_u8, 2]).unwrap();
    assert_eq!(frame.decode_with::<_, Vec<u8>>(&Json).unwrap(), vec![1, 2]);
}
