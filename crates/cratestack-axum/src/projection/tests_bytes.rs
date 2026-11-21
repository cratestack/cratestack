//! cratestack#1425: a `WireBytes` leaf — what the macros wrap every
//! `Bytes`/`Geography`/`Geometry` projection leaf in — writes a byte string
//! under CBOR and the unchanged integer array under JSON, *through*
//! `ProjectedValue`'s erased-serde bridge. That bridge forwarding
//! `is_human_readable` is the property, so it is asserted here by encoding
//! with the real codecs rather than assumed from the erased-serde source.

use std::collections::BTreeMap;

use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use cratestack_core::{CratestackCodec, WireBytes};

use super::ProjectedValue;

fn one_field(value: ProjectedValue) -> ProjectedValue {
    ProjectedValue::Object(BTreeMap::from([("data".to_owned(), value)]))
}

fn cbor(value: &ProjectedValue) -> String {
    let encoded = CborCodec.encode(value).unwrap();
    encoded.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn json(value: &ProjectedValue) -> String {
    String::from_utf8(JsonCodec.encode(value).unwrap()).unwrap()
}

#[test]
fn required_bytes_leaf_is_a_byte_string_under_cbor_and_an_array_under_json() {
    let value = one_field(ProjectedValue::leaf(WireBytes(vec![1u8, 2, 255])));
    assert_eq!(cbor(&value), "a16464617461430102ff");
    assert_eq!(json(&value), r#"{"data":[1,2,255]}"#);
}

#[test]
fn nullable_and_list_bytes_leaves_keep_their_shape() {
    let none = one_field(ProjectedValue::leaf(WireBytes(None::<Vec<u8>>)));
    assert_eq!(cbor(&none), "a16464617461f6");
    assert_eq!(json(&none), r#"{"data":null}"#);

    let some = one_field(ProjectedValue::leaf(WireBytes(Some(vec![1u8]))));
    assert_eq!(cbor(&some), "a164646174614101");

    let list = one_field(ProjectedValue::leaf(WireBytes(vec![vec![1u8], vec![]])));
    assert_eq!(cbor(&list), "a1646461746182410140");
    assert_eq!(json(&list), r#"{"data":[[1],[]]}"#);
}

#[test]
fn an_unwrapped_vec_leaf_still_writes_the_array_form() {
    // The negative control: without `WireBytes` a `Vec<u8>` leaf is the old
    // integer array, so the wrapper is what changes the wire form.
    let value = one_field(ProjectedValue::leaf(vec![1u8, 2, 255]));
    assert_eq!(cbor(&value), "a1646461746183010218ff");
}
