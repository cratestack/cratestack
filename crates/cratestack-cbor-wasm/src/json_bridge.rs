//! Bridges arbitrary `serde_json::Value` trees onto `minicbor-serde`'s
//! `Serializer` with the same null-encoding correctness the generated
//! server/client code gets from working with real `Option<T>` values.
//!
//! `encode(value: unknown)` in TypeScript has no `Option<T>` to route
//! through — any JS `null` deserializes into a plain `serde_json::Value::
//! Null`, and `serde_json::Value`'s own `Serialize` impl reports that
//! variant via `serializer.serialize_unit()`. `minicbor-serde` encodes
//! `serialize_unit()` as the CBOR empty-array marker (`0x80`), NOT the
//! null marker (`0xf6`) — exactly the wire-compatibility footgun
//! `cratestack-codec-cbor`'s own doc comments warn about (see its
//! `optional_none_round_trips_as_cbor_null` test). The macro-emitted Rust
//! server/client code never hits this because it only ever serializes
//! concrete typed structs, where `None` naturally goes through
//! `serializer.serialize_none()` (the CBOR-null path) instead.
//!
//! This module is a thin, deliberately non-wasm-bindgen wrapper so it
//! stays unit-testable on the host toolchain (no wasm32 target, no JS
//! engine) — see the tests below for the exact byte assertion.
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};
use serde_json::Value;

/// Newtype over `serde_json::Value` with a hand-written `Serialize` impl
/// that maps `Value::Null` through `serialize_none()` (CBOR null, `0xf6`)
/// at every position in the tree, not just the top level.
///
/// Its only production consumer is `wasm.rs`, which is `cfg`'d out on a
/// plain host build — so on a non-wasm32 target this type is exercised
/// exclusively by the tests below, hence the `allow`.
#[allow(dead_code)]
pub(crate) struct EncodableValue<'a>(pub(crate) &'a Value);

impl Serialize for EncodableValue<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Value::Null => serializer.serialize_none(),
            Value::Bool(value) => serializer.serialize_bool(*value),
            Value::Number(number) => number.serialize(serializer),
            Value::String(value) => serializer.serialize_str(value),
            Value::Array(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(&EncodableValue(item))?;
                }
                seq.end()
            }
            Value::Object(entries) => {
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for (key, value) in entries {
                    map.serialize_entry(key, &EncodableValue(value))?;
                }
                map.end()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use cratestack_codec_cbor::CborCodec;
    use cratestack_core::CoolCodec;
    use serde_json::{Value, json};

    use super::EncodableValue;

    fn encode(value: &serde_json::Value) -> Vec<u8> {
        CborCodec
            .encode(&EncodableValue(value))
            .expect("encode should succeed")
    }

    #[test]
    fn top_level_null_encodes_as_cbor_null_marker() {
        // The exact byte cratestack-codec-cbor's own test asserts for
        // `Option::<String>::None` — see crates/cratestack-codec-cbor/
        // src/lib.rs's `optional_none_round_trips_as_cbor_null`.
        assert_eq!(encode(&Value::Null), vec![0xf6]);
    }

    #[test]
    fn nested_null_also_encodes_as_cbor_null_marker() {
        let value = json!({ "a": null, "b": [1, null, "x"] });
        let bytes = encode(&value);
        // Decode back through the real codec and assert structurally,
        // since map key order isn't guaranteed by serde_json::Value.
        let decoded: serde_json::Value = CborCodec.decode(&bytes).expect("decode should succeed");
        assert_eq!(decoded, value);
        // And confirm the null bytes present are 0xf6 (null), never 0x80
        // (empty array) — minicbor-serde would emit the latter if we'd
        // naively serialized `serde_json::Value` directly instead of
        // through `EncodableValue`.
        assert!(bytes.contains(&0xf6));
    }

    #[test]
    fn plain_values_round_trip_through_the_real_codec() {
        let value = json!({
            "name": "cratestack",
            "count": 3,
            "tags": ["cool", "stack"],
            "active": true,
            "note": null,
        });
        let bytes = encode(&value);
        let decoded: serde_json::Value = CborCodec.decode(&bytes).expect("decode should succeed");
        assert_eq!(decoded, value);
    }

    #[test]
    fn naive_serde_json_value_serialize_would_have_produced_the_empty_array_marker() {
        // Documents *why* EncodableValue exists: serializing
        // serde_json::Value::Null directly (its own derive-free hand
        // impl calls serialize_unit()) hits minicbor-serde's non-RFC-8949
        // "unit = empty array" encoding instead of CBOR null.
        let bytes = CborCodec
            .encode(&Value::Null)
            .expect("encode should succeed");
        assert_eq!(bytes, vec![0x80]);
        assert_ne!(bytes, vec![0xf6]);
    }
}
