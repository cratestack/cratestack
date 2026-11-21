//! Wire-level tests for [`super`]: literal CBOR hex for every field shape
//! the macros emit, and JSON identity against plain serde. CBOR goes through
//! `minicbor-serde` configured the way `cratestack-codec-cbor` configures it
//! (`serialize_unit_as_null`), for the reason `lenient_bytes/tests.rs` gives.

use serde::{Deserialize, Serialize};

use super::*;

fn cbor<T: Serialize + ?Sized>(value: &T) -> String {
    let mut out = Vec::new();
    {
        let mut serializer = minicbor_serde::Serializer::new(&mut out);
        serializer.serialize_unit_as_null(true);
        value.serialize(&mut serializer).expect("cbor encode");
    }
    out.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(super) fn json<T: Serialize + ?Sized>(value: &T) -> String {
    serde_json::to_string(value).expect("json encode")
}

/// One field per `bytes_serde` table row, wire-named `data`.
#[derive(Serialize, Deserialize)]
pub(super) struct Required {
    #[serde(serialize_with = "serialize_bytes_shape")]
    pub(super) data: Vec<u8>,
}
#[derive(Serialize)]
pub(super) struct Nullable {
    #[serde(serialize_with = "serialize_bytes_shape")]
    pub(super) data: Option<Vec<u8>>,
}
#[derive(Serialize)]
pub(super) struct List {
    #[serde(serialize_with = "serialize_bytes_shape")]
    pub(super) data: Vec<Vec<u8>>,
}
#[derive(Serialize)]
pub(super) struct PatchNullable {
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_bytes_shape"
    )]
    pub(super) data: Option<Option<Vec<u8>>>,
}
#[derive(Serialize)]
pub(super) struct PatchList {
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_bytes_shape"
    )]
    pub(super) data: Option<Vec<Vec<u8>>>,
}

#[test]
fn required_is_a_byte_string_under_cbor_and_an_array_under_json() {
    let value = Required {
        data: vec![1, 2, 255],
    };
    assert_eq!(cbor(&value), "a16464617461430102ff");
    assert_eq!(json(&value), r#"{"data":[1,2,255]}"#);
    let empty = Required { data: vec![] };
    assert_eq!(cbor(&empty), "a1646461746140");
}

#[test]
fn nullable_keeps_null_and_wraps_some_as_a_byte_string() {
    assert_eq!(cbor(&Nullable { data: None }), "a16464617461f6");
    assert_eq!(
        cbor(&Nullable {
            data: Some(vec![1, 2, 255])
        }),
        "a16464617461430102ff"
    );
    assert_eq!(json(&Nullable { data: None }), r#"{"data":null}"#);
}

#[test]
fn list_writes_each_element_as_a_byte_string() {
    let value = List {
        data: vec![vec![1], vec![]],
    };
    assert_eq!(cbor(&value), "a1646461746182410140");
    assert_eq!(json(&value), r#"{"data":[[1],[]]}"#);
}

#[test]
fn patch_shapes_distinguish_untouched_cleared_and_set() {
    assert_eq!(cbor(&PatchNullable { data: None }), "a0");
    assert_eq!(cbor(&PatchNullable { data: Some(None) }), "a16464617461f6");
    assert_eq!(
        cbor(&PatchNullable {
            data: Some(Some(vec![1, 2, 255]))
        }),
        "a16464617461430102ff"
    );
    assert_eq!(
        json(&PatchNullable { data: Some(None) }),
        r#"{"data":null}"#
    );
    assert_eq!(cbor(&PatchList { data: None }), "a0");
    assert_eq!(
        cbor(&PatchList {
            data: Some(vec![vec![1]])
        }),
        "a16464617461814101"
    );
}

#[test]
fn wire_bytes_wrapper_matches_the_field_helper() {
    assert_eq!(cbor(&WireBytes(vec![1u8, 2, 255])), "430102ff");
    assert_eq!(cbor(&WireBytes(&vec![1u8])), "4101");
    assert_eq!(cbor(&WireBytes(None::<Vec<u8>>)), "f6");
    assert_eq!(json(&WireBytes(vec![1u8, 2, 255])), "[1,2,255]");
}

#[test]
fn geography_ewkb_is_a_25_byte_string() {
    let ewkb = hex("0101000020e6100000000000000000f03f0000000000000040");
    assert_eq!(ewkb.len(), 25);
    assert_eq!(
        cbor(&WireBytes(ewkb)),
        "58190101000020e6100000000000000000f03f0000000000000040"
    );
}

#[test]
fn byte_string_output_round_trips_through_the_lenient_decoder() {
    let encoded = cbor(&Required {
        data: vec![1, 2, 255],
    });
    let raw = hex(&encoded);
    let decoded: crate::lenient_bytes::LenientBytes = {
        #[derive(Deserialize)]
        struct Wrapper {
            pub(super) data: crate::lenient_bytes::LenientBytes,
        }
        minicbor_serde::from_slice::<Wrapper>(&raw).unwrap().data
    };
    assert_eq!(decoded.0, vec![1, 2, 255]);
}

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}
