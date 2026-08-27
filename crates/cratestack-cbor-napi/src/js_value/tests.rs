//! Host-runnable coverage for the *value* half of the bridge — the CBOR
//! shape each [`cratestack_core::Value`] variant lands on, and the JS
//! shape each maps to conceptually.
//!
//! The napi half (`napi_conversions.rs`) can't be exercised here: its
//! types reference `napi_*` C symbols that only exist inside a running
//! Node process, which is the whole reason the crate is split this way
//! (see the crate root docs). That half is covered by the vitest suite in
//! `packages/cratestack-cbor-node`, which loads the compiled addon in
//! real Node — including the `Uint8Array`/`Buffer`/`ArrayBuffer` cases.

use std::collections::BTreeMap;

use cratestack_codec_cbor::CborCodec;
use cratestack_core::{CratestackCodec, Value};

fn encode(value: &Value) -> Vec<u8> {
    CborCodec.encode(value).expect("encode should succeed")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn bytes_encode_as_a_cbor_byte_string_not_a_map_of_indices() {
    // cratestack#783's core assertion. Before the switch away from
    // `serde_json::Value`, a JS `Uint8Array([1,2,3,4])` reached the wire
    // as `a8 6130 01 …` — a CBOR map keyed by stringified index.
    // `0x44` is major type 2, length 4.
    assert_eq!(hex(&encode(&Value::Bytes(vec![1, 2, 3, 4]))), "4401020304");
}

#[test]
fn an_empty_byte_sequence_is_the_zero_length_byte_string() {
    // `0x40`, not `0x80` (empty array) and not `0xa0` (empty map).
    assert_eq!(hex(&encode(&Value::Bytes(Vec::new()))), "40");
}

#[test]
fn a_cbor_byte_string_decodes_back_to_bytes() {
    // The symmetric half the issue also asks for: `decode` must be able
    // to read major type 2, so a server sending a `Bytes` field that way
    // is consumable from JS.
    let decoded: Value = CborCodec
        .decode(&[0x44, 0xde, 0xad, 0xbe, 0xef])
        .expect("decode should succeed");
    assert_eq!(decoded, Value::Bytes(vec![0xde, 0xad, 0xbe, 0xef]));
}

#[test]
fn an_integer_array_is_not_reinterpreted_as_bytes() {
    // The `Array.from(bytes)` workaround callers write today must keep
    // behaving identically: an untyped value carries no schema, so
    // nothing at this layer may guess that `[1,2,3,4]` "meant" bytes.
    // Leniency belongs on the typed Rust side, where the schema says the
    // field is `Bytes` (`cratestack_core::lenient_bytes`).
    let decoded: Value = CborCodec
        .decode(&[0x84, 0x01, 0x02, 0x03, 0x04])
        .expect("decode should succeed");
    assert_eq!(
        decoded,
        Value::List(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(4)
        ])
    );
}

#[test]
fn nested_bytes_round_trip() {
    let mut entries = BTreeMap::new();
    entries.insert("nonce".to_owned(), Value::Bytes(vec![1, 2, 3]));
    entries.insert(
        "chunks".to_owned(),
        Value::List(vec![Value::Bytes(vec![4]), Value::Bytes(vec![5, 6])]),
    );
    let value = Value::Map(entries);

    let decoded: Value = CborCodec
        .decode(&encode(&value))
        .expect("decode should succeed");
    assert_eq!(decoded, value);
}

#[test]
fn non_finite_floats_survive_instead_of_collapsing_to_null() {
    // A documented consequence of dropping `serde_json::Value` from this
    // boundary: it cannot hold `NaN`/`±Infinity`, so decoding one used to
    // yield `null`. `Value::Float` keeps it, and napi turns it back into
    // the corresponding JS number.
    let nan: Value = CborCodec
        .decode(&encode(&Value::Float(f64::NAN)))
        .expect("decode should succeed");
    match nan {
        Value::Float(value) => assert!(value.is_nan(), "expected NaN, got {value}"),
        other => panic!("expected a float, got {other:?}"),
    }

    let infinity: Value = CborCodec
        .decode(&encode(&Value::Float(f64::INFINITY)))
        .expect("decode should succeed");
    assert_eq!(infinity, Value::Float(f64::INFINITY));
}
