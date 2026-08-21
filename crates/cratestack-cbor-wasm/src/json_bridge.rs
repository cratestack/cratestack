//! Bridges arbitrary `serde_json::Value` trees onto `minicbor-serde`'s
//! `Serializer` with the same null-encoding correctness the generated
//! server/client code gets from working with real `Option<T>` values.
//!
//! `encode(value: unknown)` in TypeScript has no `Option<T>` to route
//! through — any JS `null` deserializes into a plain `serde_json::Value::
//! Null`, and `serde_json::Value`'s own `Serialize` impl reports that
//! variant via `serializer.serialize_unit()`. Historically `minicbor-serde`
//! encoded `serialize_unit()` as the CBOR empty-array marker (`0x80`), NOT
//! the null marker (`0xf6`) — the wire-compatibility footgun that also hit
//! `POST /rpc/batch`'s opaque `serde_json::Value` frames (cratestack#657).
//! That gap is now closed one layer down, in
//! `cratestack_codec_cbor::CborCodec::encode` itself (it enables
//! `minicbor_serde::Serializer::serialize_unit_as_null`), so `EncodableValue`
//! no longer changes what bytes a top-level `Value::Null` produces through
//! `CborCodec` — see `naive_serde_json_value_now_encodes_correctly_via_the_codec_directly`
//! below. It's kept anyway as an explicit, self-documenting guarantee for
//! this bridge that doesn't depend on reading `CborCodec`'s internals to
//! confirm: `Value::Null` maps through `serialize_none()` at every position
//! in the tree, not just the top level, independently of whatever the
//! underlying codec does with bare `serialize_unit()`.
//!
//! This module is a thin, deliberately non-wasm-bindgen wrapper so it
//! stays unit-testable on the host toolchain (no wasm32 target, no JS
//! engine) — see the tests below for the exact byte assertions.
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
    use cratestack_core::CratestackCodec;
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
    fn naive_serde_json_value_now_encodes_correctly_via_the_codec_directly() {
        // cratestack#657 fixed the root cause one layer down:
        // `CborCodec::encode` now enables `serialize_unit_as_null`, so even
        // a raw `serde_json::Value::Null` — no `EncodableValue` wrapper
        // involved — encodes as CBOR null. This used to assert the old,
        // buggy `0x80` byte to document *why* `EncodableValue` was needed;
        // now that the codec handles it directly, `EncodableValue` is
        // redundant-but-harmless defense in depth (see the module doc)
        // rather than load-bearing for this specific case.
        let bytes = CborCodec
            .encode(&Value::Null)
            .expect("encode should succeed");
        assert_eq!(bytes, vec![0xf6]);
    }
}
