//! Wire-shape coverage for `Value`'s untagged `Serialize`/`Deserialize`.
//!
//! These tests pin the *observable bytes*, not just round-trip equality —
//! a round-trip test alone would still pass if the tag came back, since a
//! tagged encoder and a tagged decoder agree with each other perfectly.

use std::collections::BTreeMap;

use serde_json::json;

use super::Value;

fn map_of(pairs: &[(&str, Value)]) -> Value {
    Value::Map(
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect::<BTreeMap<_, _>>(),
    )
}

// ── the regression this codec exists for ────────────────────────────────

#[test]
fn string_serializes_bare_not_externally_tagged() {
    // The exact shape consumers had to hand-write before this change:
    // `{"String":"foo"}`. It must never come back.
    let encoded = serde_json::to_value(Value::String("foo".to_owned())).unwrap();
    assert_eq!(encoded, json!("foo"));
    assert_ne!(encoded, json!({ "String": "foo" }));
}

#[test]
fn empty_map_serializes_as_bare_object() {
    let encoded = serde_json::to_value(Value::Map(BTreeMap::new())).unwrap();
    assert_eq!(encoded, json!({}));
    assert_ne!(encoded, json!({ "Map": {} }));
}

#[test]
fn list_serializes_as_bare_array() {
    let value = Value::List(vec![Value::Int(1), Value::String("a".to_owned())]);
    assert_eq!(serde_json::to_value(value).unwrap(), json!([1, "a"]));
}

#[test]
fn wire_shape_matches_to_plain_json_exactly() {
    // The persisted shape and the wire shape are now the same thing; this
    // is the property the whole change is for. Bytes are excluded — they
    // are the one variant where the two paths legitimately differ by format
    // (see `cbor_bytes_are_a_native_byte_string`).
    let value = Value::List(vec![
        map_of(&[("nested", Value::Bool(true)), ("n", Value::Int(-7))]),
        Value::Float(1.5),
        Value::Null,
        Value::String("x".to_owned()),
    ]);
    assert_eq!(serde_json::to_value(&value).unwrap(), value.to_plain_json());
}

// ── JSON round-trips ────────────────────────────────────────────────────

#[test]
fn json_round_trips_every_variant_except_bytes() {
    for value in [
        Value::Null,
        Value::Bool(true),
        Value::Bool(false),
        Value::Int(0),
        Value::Int(-42),
        Value::Int(i64::MAX),
        Value::Int(i64::MIN),
        Value::Float(1.5),
        Value::String(String::new()),
        Value::String("héllo ⚙".to_owned()),
        Value::List(vec![]),
        Value::List(vec![Value::Int(1), Value::Null]),
        Value::Map(BTreeMap::new()),
        map_of(&[("a", Value::Int(1)), ("b", Value::List(vec![Value::Null]))]),
    ] {
        let encoded = serde_json::to_string(&value).unwrap();
        let decoded: Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, value, "json round-trip failed for {value:?}");
    }
}

#[test]
fn json_decodes_what_any_other_producer_writes() {
    // Plain JSON from a non-cratestack writer must decode — this is the
    // cratestack#162 complaint, now true on the wire as well as on disk.
    let decoded: Value = serde_json::from_str(r#"{"models":["gpt-4","gpt-4o"]}"#).unwrap();
    assert_eq!(
        decoded,
        map_of(&[(
            "models",
            Value::List(vec![
                Value::String("gpt-4".to_owned()),
                Value::String("gpt-4o".to_owned()),
            ])
        )])
    );
}

#[test]
fn json_bytes_are_base64_and_decode_back_as_string() {
    // Same documented asymmetry `to_plain_json` already carries: JSON has
    // no byte type, and nothing distinguishes base64 from ordinary text.
    let value = Value::Bytes(vec![0xDE, 0xAD, 0xBE, 0xEF]);
    let encoded = serde_json::to_value(&value).unwrap();
    assert_eq!(encoded, json!("3q2+7w=="));
    assert_eq!(encoded, value.to_plain_json());

    let decoded: Value = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded, Value::String("3q2+7w==".to_owned()));
}

// ── CBOR round-trips (the format that actually ships) ───────────────────
//
// `minicbor-serde` is the first-party CBOR backend (`cratestack-codec-cbor`).
// It is a dev-dependency here so core can pin its own wire shape without
// depending on the codec crate, which would be a cycle.

#[test]
fn cbor_round_trips_every_variant_including_bytes() {
    for value in [
        Value::Null,
        Value::Bool(true),
        Value::Int(0),
        Value::Int(-42),
        Value::Int(i64::MAX),
        Value::Int(i64::MIN),
        Value::Float(1.5),
        Value::String("héllo ⚙".to_owned()),
        Value::Bytes(vec![]),
        Value::Bytes(vec![0xDE, 0xAD, 0xBE, 0xEF]),
        Value::List(vec![]),
        Value::List(vec![Value::Int(1), Value::Null]),
        Value::Map(BTreeMap::new()),
        map_of(&[("a", Value::Bytes(vec![1, 2]))]),
    ] {
        let encoded = minicbor_serde::to_vec(&value).unwrap();
        let decoded: Value = minicbor_serde::from_slice(&encoded).unwrap();
        assert_eq!(decoded, value, "cbor round-trip failed for {value:?}");
    }
}

#[test]
fn cbor_null_is_rfc8949_null_not_an_empty_array() {
    // `minicbor-serde` encodes `()` as 0x80 (an empty array) but `None` as
    // 0xf6 (null). `Value::Null` must take the second path, or every null
    // nested in a list goes on the wire non-conformant. This is why the
    // impl calls `serialize_none` and not `serialize_unit`.
    assert_eq!(minicbor_serde::to_vec(&Value::Null).unwrap(), vec![0xf6]);
}

#[test]
fn cbor_bytes_are_a_native_byte_string() {
    // 0x44 = major type 2 (byte string), length 4. Not an array of ints,
    // which is what a plain `Vec<u8>` would have produced.
    assert_eq!(
        minicbor_serde::to_vec(Value::Bytes(vec![0xDE, 0xAD, 0xBE, 0xEF])).unwrap(),
        vec![0x44, 0xDE, 0xAD, 0xBE, 0xEF]
    );
}

#[test]
fn cbor_string_is_bare() {
    // 0x63 = major type 3 (text string), length 3 — then "foo". A tagged
    // encoding would have produced a map wrapping it.
    assert_eq!(
        minicbor_serde::to_vec(Value::String("foo".to_owned())).unwrap(),
        vec![0x63, b'f', b'o', b'o']
    );
}

#[test]
fn cbor_unsigned_integers_decode_as_int() {
    // CBOR encodes small non-negative integers with major type 0, so they
    // arrive through `visit_u64` rather than `visit_i64`.
    let encoded = minicbor_serde::to_vec(Value::Int(7)).unwrap();
    let decoded: Value = minicbor_serde::from_slice(&encoded).unwrap();
    assert_eq!(decoded, Value::Int(7));
}

#[test]
fn oversized_unsigned_degrades_to_float_rather_than_erroring() {
    // u64 past i64::MAX has no `Value::Int` representation. Matching
    // `from_plain_json`'s policy for oversized JSON numbers, it degrades
    // instead of failing the whole decode.
    let encoded = minicbor_serde::to_vec(i64::MAX as u64 + 1).unwrap();
    let decoded: Value = minicbor_serde::from_slice(&encoded).unwrap();
    assert!(matches!(decoded, Value::Float(_)), "got {decoded:?}");
}

// ── nesting ─────────────────────────────────────────────────────────────

#[test]
fn deeply_nested_structures_survive_both_formats() {
    let value = map_of(&[
        (
            "meta",
            map_of(&[
                ("tags", Value::List(vec![Value::String("a".to_owned())])),
                ("count", Value::Int(2)),
                ("missing", Value::Null),
            ]),
        ),
        ("flag", Value::Bool(false)),
    ]);

    let json_encoded = serde_json::to_string(&value).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&json_encoded).unwrap(),
        value.clone()
    );

    let cbor_encoded = minicbor_serde::to_vec(&value).unwrap();
    assert_eq!(
        minicbor_serde::from_slice::<Value>(&cbor_encoded).unwrap(),
        value
    );
}
