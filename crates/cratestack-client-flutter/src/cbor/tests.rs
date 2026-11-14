//! Unit tests for the CBOR bridge (`super`).

use serde_json::json;

use super::*;

#[test]
fn content_type_matches_codec_constant() {
    assert_eq!(CborCodec::CONTENT_TYPE, "application/cbor");
    assert_eq!(content_type(), CborCodec::CONTENT_TYPE);
}

#[test]
fn encode_then_decode_round_trips_through_the_json_text_wrapper_functions() {
    // Exercises the exact logic frb's `encode_json`/`decode_json`
    // would delegate to once codegen wraps them (not just
    // `CborCodec` in isolation).
    let json = r#"{"cratestack":["cool","stack"],"n":42}"#;
    let bytes = encode_json(json.to_owned()).expect("encode should succeed");
    let decoded_json = decode_json(bytes).expect("decode should succeed");
    let decoded: Value = serde_json::from_str(&decoded_json).unwrap();
    assert_eq!(decoded, json!({"cratestack": ["cool", "stack"], "n": 42}));
}

#[test]
fn top_level_none_round_trips_as_cbor_null() {
    // Proves this wrapper preserves cratestack-codec-cbor's
    // documented `Option::None` -> CBOR null (`0xf6`) behavior, not
    // the `Value::Null`-via-`serialize_unit` empty-array (`0x80`)
    // quirk its own test suite warns about.
    let bytes = encode_value(&Value::Null).expect("encode null should succeed");
    assert_eq!(bytes, vec![0xf6]);
    let decoded = decode_bytes(&bytes).expect("decode should succeed");
    assert_eq!(decoded, Value::Null);
}

#[test]
fn malformed_cbor_bytes_return_an_error_not_a_panic() {
    // 0x1b announces an 8-byte unsigned integer but supplies none —
    // truncated/malformed input `minicbor` rejects with an error
    // rather than panicking.
    let malformed = vec![0x1b];
    let result = decode_json(malformed);
    assert!(result.is_err(), "malformed CBOR must error, not panic");
}

#[test]
fn deeply_nested_cbor_returns_an_error_not_a_stack_overflow() {
    // cratestack#1315: Dart's native `decodeJson` goes through this path.
    let mut bytes = vec![0x81; 100_000];
    bytes.push(0x01);
    assert!(decode_json(bytes).is_err());
}

#[test]
fn invalid_json_text_returns_an_error_not_a_panic() {
    let result = encode_json("{not valid json".to_owned());
    assert!(
        result.is_err(),
        "malformed JSON input must error, not panic"
    );
}

#[test]
fn fixture_bytes_shared_with_the_napi_and_wasm_cross_language_tests_stay_correct() {
    // Same exact hex fixtures as `cratestack-cbor-napi`'s
    // `fixture_bytes_shared_with_the_js_cross_language_test_stay_correct`
    // (`crates/cratestack-cbor-napi/src/lib.rs`) — proving this
    // bridge produces byte-identical CBOR to the Node addon (and,
    // transitively, the wasm build, which shares the same fixtures)
    // for the same JSON input. Three independent bindings asserting
    // the same wire bytes from three ends of the FFI boundary is what
    // "byte-identical to the Rust CborCodec" actually proves.
    assert_eq!(
        hex(&encode_value(&json!(["cool", "stack"])).expect("encode")),
        "8264636f6f6c65737461636b"
    );
    assert_eq!(
        hex(
            &encode_value(&json!({"cratestack": ["cool", "stack"], "n": 42, "ok": true}))
                .expect("encode")
        ),
        "a36a6372617465737461636b8264636f6f6c65737461636b616e182a626f6bf5"
    );
    assert_eq!(
        hex(&encode_value(&json!({"a": null, "b": [1, null, "x"]})).expect("encode")),
        "a26161f661628301f66178"
    );
}

/// ADR 0019 D2: a `BigInt` travels as a CBOR text string (major type
/// 3) holding the canonical decimal form. Tuples are
/// `(decimal, exact CBOR hex of {"amountE8": <decimal>})`, the same
/// five strings as `cratestack-cbor-napi`'s `lib.rs` (which documents
/// where every copy lives and how the hex was derived).
const BIGINT_FIXTURES: [(&str, &str); 5] = [
    (
        "9223372036854775807",
        "a168616d6f756e7445387339323233333732303336383534373735383037",
    ),
    (
        "-9223372036854775808",
        "a168616d6f756e744538742d39323233333732303336383534373735383038",
    ),
    (
        "9007199254740993",
        "a168616d6f756e7445387039303037313939323534373430393933",
    ),
    ("0", "a168616d6f756e7445386130"),
    ("-1", "a168616d6f756e744538622d31"),
];

fn unhex(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("valid hex"))
        .collect()
}

#[test]
fn bigint_text_string_fixtures_stay_byte_exact_through_the_json_text_boundary() {
    // The Dart side hands this bridge `jsonEncode(...)` text, so the
    // boundary is exercised exactly as `cratestack_cbor` does: JSON
    // text in, bytes out, bytes in, JSON text out. A decimal string
    // must stay a string at every hop.
    for (decimal, expected_hex) in BIGINT_FIXTURES {
        let json_text = format!(r#"{{"amountE8":"{decimal}"}}"#);

        let bytes = encode_json(json_text.clone()).expect("encode");
        assert_eq!(hex(&bytes), expected_hex, "encode {decimal}");
        // `0xa1` (map of one), `0x68` + 8 key bytes, then the value
        // header at index 10: major type 3, never 0/1 (integer).
        assert_eq!(bytes[10] >> 5, 3, "{decimal} must be a text string");

        let decoded = decode_json(unhex(expected_hex)).expect("decode");
        assert_eq!(decoded, json_text, "decode {decimal}");
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

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
