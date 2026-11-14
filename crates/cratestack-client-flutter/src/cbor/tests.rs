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

pub(super) fn unhex(hex: &str) -> Vec<u8> {
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
