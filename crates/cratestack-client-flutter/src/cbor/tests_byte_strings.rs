//! Byte-string and legacy-parity tests for the CBOR bridge (`super`), cratestack#1359.

use serde_json::json;

use super::tests::unhex;
use super::*;

/// What `decode_bytes` was before cratestack#1359: straight into
/// `serde_json::Value`, which has no byte-string arm.
fn legacy_decode(bytes: &[u8]) -> Result<Value, CratestackError> {
    CborCodec.decode::<Value>(bytes)
}

fn unhex_str(hex: &str) -> Vec<u8> {
    unhex(&hex.replace(' ', ""))
}

#[test]
fn frame_value_decode_matches_the_legacy_json_value_decode() {
    // Everything the old decode accepted must decode identically (or be
    // refused identically) now, so this is a corpus of the edge cases where
    // two `Value`-shaped visitors could plausibly diverge.
    let corpus = [
        ("null", "f6"),
        ("true", "f5"),
        ("small int", "18 2a"),
        ("negative int", "29"),
        ("u64::MAX", "1b ffffffffffffffff"),
        ("i64::MIN", "3b 7fffffffffffffff"),
        ("below i64::MIN (-2^64)", "3b ffffffffffffffff"),
        ("f64", "fb 3ff8000000000000"),
        ("f32", "fa 3fc00000"),
        ("f16", "f9 3e00"),
        ("NaN", "f9 7e00"),
        ("+inf", "f9 7c00"),
        ("undefined", "f7"),
        ("simple(16)", "f0"),
        ("text", "63 616263"),
        ("empty text", "60"),
        ("array", "83 01 02 03"),
        ("empty map", "a0"),
        ("map with int key", "a1 01 02"),
        ("duplicate keys keep the last", "a2 61 61 01 61 61 02"),
        ("nested", "a2 61 61 82 01 a1 61 62 f6 61 63 f5"),
        ("indefinite text", "7f 61 61 61 62 ff"),
        ("indefinite array", "9f 01 02 ff"),
        ("indefinite map", "bf 61 61 01 ff"),
        ("tag 1 epoch", "c1 1a 514b67b0"),
        (
            "tag 0 date text",
            "c0 74 323031332d30332d32315432303a30343a30305a",
        ),
        ("positive bignum", "c2 49 010000000000000000"),
        ("negative bignum", "c3 49 010000000000000000"),
        ("tag 32 uri", "d8 20 63 616263"),
        ("tag over a map", "c7 a0"),
        ("truncated", "1b"),
        ("trailing bytes", "01 02"),
    ];
    for (name, hex) in corpus {
        let bytes = unhex_str(hex);
        let old = legacy_decode(&bytes).map_err(|error| error.to_string());
        let new = decode_bytes(&bytes).map_err(|error| error.to_string());
        match (&old, &new) {
            (Ok(old), Ok(new)) => assert_eq!(old, new, "{name}: decoded differently"),
            (Err(_), Err(_)) => {}
            _ => panic!("{name}: legacy {old:?} vs frame-value {new:?}"),
        }
    }
}

#[test]
fn a_byte_string_decodes_to_an_integer_array_instead_of_failing_the_body() {
    // cratestack#1359. `{"id": h'67e55044...', "maybe": null, "ids": [h'..', h'..']}`,
    // the shape the server writes for a struct of `Uuid`s.
    let id = "67e5504410b1426f9247bb680e5fe0c8";
    let bytes = unhex_str(&format!(
        "a3 62 6964 50 {id} 65 6d61796265 f6 63 696473 82 50 {id} 50 {id}"
    ));
    assert!(
        legacy_decode(&bytes).is_err(),
        "the old decode refused the whole body: that is the bug"
    );
    let ints: Vec<u8> = unhex_str(id);
    let expected = json!({"id": ints, "maybe": null, "ids": [ints, ints]});
    let decoded: Value = serde_json::from_str(&decode_json(bytes).expect("decode")).unwrap();
    assert_eq!(decoded, expected);
}

#[test]
fn a_non_uuid_byte_string_is_an_array_too() {
    let decoded = decode_bytes(&unhex_str("44 01020304")).expect("decode");
    assert_eq!(decoded, json!([1, 2, 3, 4]));
}

#[test]
fn the_pinned_dart_byte_string_fixtures_decode_as_the_dart_tests_expect() {
    // The same hex and expected JSON as
    // `dart-packages/cratestack_cbor/test/shared_fixtures.dart`
    // (`uuidByteStringHex` / `uuidNestedByteStringHex`), which both Dart
    // backends assert, so the three agree on one wire contract.
    let flat = decode_json(unhex_str("a1626964500123456789abcdef0123456789abcdef")).unwrap();
    assert_eq!(
        flat,
        r#"{"id":[1,35,69,103,137,171,205,239,1,35,69,103,137,171,205,239]}"#
    );
    let nested = decode_json(unhex_str(
        "a16369647382500123456789abcdef0123456789abcdeff6",
    ))
    .unwrap();
    assert_eq!(
        nested,
        r#"{"ids":[[1,35,69,103,137,171,205,239,1,35,69,103,137,171,205,239],null]}"#
    );
}
