//! Host-testable byte assertions for the JS value type this crate's
//! `encode`/`decode` funnel every JS value through:
//! [`cratestack_core::Value`].
//!
//! **Why `Value` and not `serde_json::Value`.** `serde_json::Value` has no
//! byte-string variant, so a JS `Uint8Array` reaching it could only ever
//! become an object keyed by index — `{"0":1,"1":2,…}` on the wire, which
//! no `Vec<u8>` on the Rust side can decode and which costs ~7x the bytes
//! of the byte string it should have been (cratestack#783). `Value` has a
//! `Bytes` variant whose `Serialize` impl branches on
//! `is_human_readable()`: `minicbor-serde` reports `false`, so it takes
//! the `serialize_bytes` branch and lands on the wire as RFC 8949 major
//! type 2. Its `Deserialize` closes the same loop inbound, via
//! `visit_bytes`/`visit_byte_buf`.
//!
//! `serde-wasm-bindgen` handles the JS half of that: its `deserialize_any`
//! recognises `Uint8Array` and `ArrayBuffer` and calls `visit_byte_buf`
//! (`de.rs`'s `as_bytes`), and its `Serializer` turns `serialize_bytes`
//! back into a `Uint8Array` unless `serialize_bytes_as_arrays` is set —
//! which `wasm.rs` deliberately leaves at its default.
//!
//! **What replaced the old `EncodableValue` shim.** This module used to
//! hold a hand-written `Serialize` for `serde_json::Value` that routed
//! `Value::Null` through `serialize_none()` (CBOR `0xf6`) instead of
//! `serialize_unit()` (which `minicbor-serde` once rendered as the empty
//! array `0x80` — cratestack#657). `cratestack_core::Value` does that
//! natively (`crate::value::codec`'s `Serialize` impl calls
//! `serialize_none` for `Value::Null`, for exactly that reason), so that
//! part of the shim had nothing left to do. What remains is
//! [`JsSerializable`], which exists for the *opposite* reason — see its
//! docs. The byte assertions the old shim carried are kept below, since
//! they are this crate's only coverage that runs without a wasm
//! toolchain.

use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};

use cratestack_core::Value;

/// Wrapper that pins `Value::Bytes` to `serialize_bytes` no matter what
/// the target serializer reports for `is_human_readable()`.
///
/// `Value`'s own `Serialize` deliberately branches on that flag: binary
/// formats get a native byte string, human-readable ones get base64,
/// because JSON has no byte type. `serde_wasm_bindgen::Serializer`
/// inherits serde's `is_human_readable() == true` default and has no
/// switch for it, so decoding a CBOR byte string straight through it
/// produced a base64 *string* in JS — the one shape the caller can't tell
/// apart from ordinary text, and not the `Uint8Array` cratestack#783 asks
/// for. JS has a first-class binary type, so the human-readable branch is
/// simply wrong at this boundary.
///
/// Every other variant is delegated unchanged; the recursion exists only
/// to reach nested `Bytes`.
#[allow(dead_code)] // Only `wasm.rs` (cfg'd to wasm32) uses this in anger.
pub(crate) struct JsSerializable<'a>(pub(crate) &'a Value);

impl Serialize for JsSerializable<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Value::Bytes(bytes) => serializer.serialize_bytes(bytes),
            Value::List(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(&JsSerializable(item))?;
                }
                seq.end()
            }
            Value::Map(entries) => {
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for (key, value) in entries {
                    map.serialize_entry(key, &JsSerializable(value))?;
                }
                map.end()
            }
            scalar => scalar.serialize(serializer),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use cratestack_codec_cbor::CborCodec;
    use cratestack_core::{CratestackCodec, Value};

    fn encode(value: &Value) -> Vec<u8> {
        CborCodec.encode(value).expect("encode should succeed")
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn map(entries: &[(&str, Value)]) -> Value {
        Value::Map(
            entries
                .iter()
                .map(|(key, value)| ((*key).to_owned(), value.clone()))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    #[test]
    fn bytes_encode_as_a_cbor_byte_string() {
        // `0x44` = major type 2, length 4. The whole point of
        // cratestack#783: this is what a JS `Uint8Array` must reach the
        // wire as, and what a generated Rust `Bytes` field now accepts.
        assert_eq!(hex(&encode(&Value::Bytes(vec![1, 2, 3, 4]))), "4401020304");
        // `0x40` — the zero-length byte string, not an empty array.
        assert_eq!(hex(&encode(&Value::Bytes(Vec::new()))), "40");
    }

    #[test]
    fn bytes_round_trip_through_the_real_codec() {
        let value = map(&[
            ("nonce", Value::Bytes(vec![0xde, 0xad, 0xbe, 0xef])),
            ("label", Value::String("mailbox".to_owned())),
        ]);
        let decoded: Value = CborCodec
            .decode(&encode(&value))
            .expect("decode should succeed");
        assert_eq!(decoded, value);
    }

    #[test]
    fn a_list_of_integers_still_decodes_as_a_list() {
        // The shape a TypeScript caller doing the `Array.from(bytes)`
        // workaround sends. It must keep decoding as a list of integers,
        // not be silently reinterpreted as bytes — the leniency belongs
        // on the typed Rust side (`cratestack_core::lenient_bytes`),
        // where the schema says the field is `Bytes`; an untyped `Value`
        // has no such information and must not guess.
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
    fn top_level_null_encodes_as_the_cbor_null_marker() {
        // Was `EncodableValue`'s reason to exist (cratestack#657); now a
        // property of `Value` itself. Pinned here so replacing that shim
        // can't have quietly regressed it.
        assert_eq!(encode(&Value::Null), vec![0xf6]);
    }

    #[test]
    fn nested_null_also_encodes_as_the_cbor_null_marker() {
        let value = map(&[
            ("a", Value::Null),
            (
                "b",
                Value::List(vec![
                    Value::Int(1),
                    Value::Null,
                    Value::String("x".to_owned()),
                ]),
            ),
        ]);
        let bytes = encode(&value);
        let decoded: Value = CborCodec.decode(&bytes).expect("decode should succeed");
        assert_eq!(decoded, value);
        assert!(bytes.contains(&0xf6), "expected a CBOR null marker");
    }

    #[test]
    fn plain_values_are_byte_identical_to_the_previous_serde_json_bridge() {
        // Switching the bridge type is only safe if it changes nothing
        // for values that have no bytes in them. These three fixtures are
        // the ones `cratestack-cbor-napi`'s cross-language test and
        // `packages/cratestack-cbor-node`'s vitest suite both hardcode —
        // asserting them here proves the swap is a no-op on the existing
        // wire, not just that the new path round-trips with itself.
        assert_eq!(
            hex(&encode(&Value::List(vec![
                Value::String("cool".to_owned()),
                Value::String("stack".to_owned()),
            ]))),
            "8264636f6f6c65737461636b"
        );
        assert_eq!(
            hex(&encode(&map(&[
                (
                    "cratestack",
                    Value::List(vec![
                        Value::String("cool".to_owned()),
                        Value::String("stack".to_owned()),
                    ])
                ),
                ("n", Value::Int(42)),
                ("ok", Value::Bool(true)),
            ]))),
            "a36a6372617465737461636b8264636f6f6c65737461636b616e182a626f6bf5"
        );
        assert_eq!(
            hex(&encode(&map(&[
                ("a", Value::Null),
                (
                    "b",
                    Value::List(vec![
                        Value::Int(1),
                        Value::Null,
                        Value::String("x".to_owned()),
                    ])
                ),
            ]))),
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
    fn bigint_text_string_fixtures_stay_byte_exact_in_both_directions() {
        // A text string is just a string to this bridge, so a `BigInt`
        // needs no bridge change; this pins that it stays true. The
        // `@cratestack/cbor-web` vitest suite asserts the same bytes
        // through the real wasm-bindgen build.
        for (decimal, expected_hex) in BIGINT_FIXTURES {
            let input = map(&[("amountE8", Value::String(decimal.to_owned()))]);

            let bytes = encode(&input);
            assert_eq!(hex(&bytes), expected_hex, "encode {decimal}");
            // `0xa1` (map of one), `0x68` + 8 key bytes, then the value
            // header at index 10: major type 3, never 0/1 (integer).
            assert_eq!(bytes[10] >> 5, 3, "{decimal} must be a text string");

            let decoded: Value = CborCodec
                .decode(&unhex(expected_hex))
                .expect("decode should succeed");
            assert_eq!(decoded, input, "decode {decimal}");
        }
    }
}
