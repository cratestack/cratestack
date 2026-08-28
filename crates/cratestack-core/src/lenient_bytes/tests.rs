//! Wire-level tests for [`super`]. Every CBOR assertion goes through
//! `minicbor-serde` — the backend `cratestack-codec-cbor` wraps — for the
//! reason its dev-dependency comment gives: depending on the codec crate
//! from core would be a cycle, and the wire shape is core's contract.
//!
//! `minicbor_serde::from_slice` is the *unconfigured* deserializer, which
//! is exactly what `CborCodec::decode` uses (that codec only customizes
//! the serializer). So a byte sequence accepted here is accepted by the
//! real server decode path.

use serde::Deserialize;

use super::*;

/// `0xa1 0x6170` — a one-entry map with the key `"p"`.
const MAP_P: [u8; 3] = [0xa1, 0x61, 0x70];

fn cbor(payload: &[u8]) -> Vec<u8> {
    let mut bytes = MAP_P.to_vec();
    bytes.extend_from_slice(payload);
    bytes
}

/// `0x44 01020304` — CBOR major type 2, a 4-byte byte string. What
/// `@cratestack/cbor` now emits for a JS `Uint8Array`.
const BYTE_STRING: [u8; 5] = [0x44, 0x01, 0x02, 0x03, 0x04];

/// `0x84 01 02 03 04` — a 4-element array of integers. What every
/// generated client emits today.
const INT_ARRAY: [u8; 5] = [0x84, 0x01, 0x02, 0x03, 0x04];

#[derive(Debug, Deserialize, PartialEq)]
struct Required {
    #[serde(deserialize_with = "deserialize_bytes")]
    p: Vec<u8>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct Optional {
    #[serde(default, deserialize_with = "deserialize_optional_bytes")]
    p: Option<Vec<u8>>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct ListArity {
    #[serde(deserialize_with = "deserialize_bytes_list")]
    p: Vec<Vec<u8>>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct PatchedList {
    #[serde(default, deserialize_with = "deserialize_optional_bytes_list")]
    p: Option<Vec<Vec<u8>>>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct PatchedNullable {
    #[serde(default, deserialize_with = "deserialize_double_option_bytes")]
    p: Option<Option<Vec<u8>>>,
}

#[test]
fn stock_vec_u8_rejects_a_byte_string() {
    // The bug this module exists for, pinned so the fix can't be quietly
    // reverted into a passing suite: without `deserialize_with`, a field
    // typed `Vec<u8>` cannot decode CBOR major type 2 at all. This is the
    // 400 `invalid request payload` cratestack#783 reports.
    #[derive(Debug, Deserialize)]
    struct Stock {
        #[allow(dead_code)]
        p: Vec<u8>,
    }

    let error = minicbor_serde::from_slice::<Stock>(&cbor(&BYTE_STRING))
        .expect_err("stock Vec<u8> must reject a CBOR byte string");
    assert!(
        error.to_string().contains("bytes"),
        "expected a bytes/array type mismatch, got: {error}"
    );
}

#[test]
fn required_accepts_both_wire_shapes() {
    let from_byte_string: Required =
        minicbor_serde::from_slice(&cbor(&BYTE_STRING)).expect("byte string should decode");
    let from_int_array: Required =
        minicbor_serde::from_slice(&cbor(&INT_ARRAY)).expect("int array should decode");

    assert_eq!(from_byte_string.p, vec![1, 2, 3, 4]);
    assert_eq!(from_byte_string, from_int_array);
}

#[test]
fn required_accepts_the_json_array_shape() {
    // JSON has no byte-string type, so the integer-array form is the only
    // shape the `application/json` transport can express — this asserts
    // the custom `deserialize_with` didn't break it.
    let parsed: Required = serde_json::from_str(r#"{"p":[1,2,3,4]}"#).expect("json should decode");
    assert_eq!(parsed.p, vec![1, 2, 3, 4]);
}

#[test]
fn empty_byte_string_decodes_as_an_empty_vec() {
    // `0x40` — a zero-length byte string. Distinct code path from a
    // populated one in `minicbor`'s decoder, and the shape
    // `encode(new Uint8Array())` produces.
    let parsed: Required = minicbor_serde::from_slice(&cbor(&[0x40])).expect("empty byte string");
    assert_eq!(parsed.p, Vec::<u8>::new());
}

#[test]
fn optional_accepts_both_shapes_plus_null_and_absent() {
    let from_byte_string: Optional =
        minicbor_serde::from_slice(&cbor(&BYTE_STRING)).expect("byte string should decode");
    assert_eq!(from_byte_string.p, Some(vec![1, 2, 3, 4]));

    let from_int_array: Optional =
        minicbor_serde::from_slice(&cbor(&INT_ARRAY)).expect("int array should decode");
    assert_eq!(from_int_array.p, Some(vec![1, 2, 3, 4]));

    // `0xf6` — CBOR null.
    let from_null: Optional =
        minicbor_serde::from_slice(&cbor(&[0xf6])).expect("null should decode");
    assert_eq!(from_null.p, None);

    // `0xa0` — an empty map. `#[serde(default)]` has to carry this case,
    // because a custom `deserialize_with` opts the field out of
    // serde-derive's implicit missing-`Option`-is-`None` handling.
    let absent: Optional = minicbor_serde::from_slice(&[0xa0]).expect("absent key should decode");
    assert_eq!(absent.p, None);
}

#[test]
fn list_arity_accepts_a_mix_of_both_element_shapes() {
    // `0x82` — a 2-element array holding one byte string and one int
    // array. Element shape is decided per element, not per field.
    let mut payload = vec![0x82];
    payload.extend_from_slice(&BYTE_STRING);
    payload.extend_from_slice(&INT_ARRAY);

    let parsed: ListArity =
        minicbor_serde::from_slice(&cbor(&payload)).expect("list should decode");
    assert_eq!(parsed.p, vec![vec![1, 2, 3, 4], vec![1, 2, 3, 4]]);
}

#[test]
fn patched_list_accepts_both_shapes_and_an_absent_key() {
    let mut payload = vec![0x81];
    payload.extend_from_slice(&BYTE_STRING);
    let parsed: PatchedList =
        minicbor_serde::from_slice(&cbor(&payload)).expect("patched list should decode");
    assert_eq!(parsed.p, Some(vec![vec![1, 2, 3, 4]]));

    let absent: PatchedList =
        minicbor_serde::from_slice(&[0xa0]).expect("absent key should decode");
    assert_eq!(absent.p, None);
}

#[test]
fn patched_nullable_keeps_the_three_way_distinction() {
    // The cratestack#567 contract, re-proved for `Bytes`: absent, an
    // explicit null ("clear this column"), and a value must stay three
    // distinguishable states — and the value form must accept both wire
    // shapes.
    let absent: PatchedNullable =
        minicbor_serde::from_slice(&[0xa0]).expect("absent key should decode");
    assert_eq!(absent.p, None);

    let cleared: PatchedNullable =
        minicbor_serde::from_slice(&cbor(&[0xf6])).expect("explicit null should decode");
    assert_eq!(cleared.p, Some(None));

    let from_byte_string: PatchedNullable =
        minicbor_serde::from_slice(&cbor(&BYTE_STRING)).expect("byte string should decode");
    assert_eq!(from_byte_string.p, Some(Some(vec![1, 2, 3, 4])));

    let from_int_array: PatchedNullable =
        minicbor_serde::from_slice(&cbor(&INT_ARRAY)).expect("int array should decode");
    assert_eq!(from_int_array.p, Some(Some(vec![1, 2, 3, 4])));
}

#[test]
fn an_out_of_range_element_still_errors() {
    // `0x81 0x18 0xff` is `[255]` (in range); `0x81 0x19 0x01 0x00` is
    // `[256]`, which is not a byte. Leniency is about the container
    // shape, not about silently truncating values.
    let ok: Required =
        minicbor_serde::from_slice(&cbor(&[0x81, 0x18, 0xff])).expect("255 is a valid byte");
    assert_eq!(ok.p, vec![255]);

    assert!(
        minicbor_serde::from_slice::<Required>(&cbor(&[0x81, 0x19, 0x01, 0x00])).is_err(),
        "256 must not decode as a byte"
    );
}

#[test]
fn a_wrong_type_still_errors() {
    // `0x63 616263` — the text string "abc". Neither shape; must not be
    // silently reinterpreted as its UTF-8 bytes.
    assert!(
        minicbor_serde::from_slice::<Required>(&cbor(&[0x63, 0x61, 0x62, 0x63])).is_err(),
        "a text string must not decode as Bytes"
    );
}
