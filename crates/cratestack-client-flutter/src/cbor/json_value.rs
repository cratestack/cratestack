//! JSON-to-CBOR serialization shim for [`super::encode_value`].
//!
//! Duplicated (not shared) from `cratestack-cbor-napi`'s `json_value.rs`
//! and `cratestack-cbor-wasm`'s `json_bridge.rs` — the "small pure
//! mapping table gets reimplemented per crate" convention
//! `cratestack-client-dart::grpc::wire` already documents for itself,
//! rather than introducing a shared dependency edge between three
//! otherwise-independent platform-binding crates for ~20 lines of logic.
//!
//! `cratestack-codec-cbor`'s `CborCodec` (which this crate wraps
//! unchanged — see `super`'s module docs) is generic over any
//! `T: Serialize`. Feeding it a `serde_json::Value` directly works for
//! every JSON shape *except* `null`: `Value::Null`'s `Serialize` impl
//! calls `serializer.serialize_unit()`, and `minicbor-serde`'s default
//! `Serializer` renders a Rust unit `()` as the CBOR empty-array marker
//! (`0x80`), not the CBOR null marker (`0xf6`) — the exact non-RFC-8949
//! quirk `CborCodec::encode`'s own doc comment warns about. A JSON-text
//! entry point has no `Option<T>` to route through — every JSON `null`
//! (top-level or nested) has to go through this wrapper instead, or it
//! would silently round-trip as `[]` once decoded back.

use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};
use serde_json::Value;

/// Borrowing wrapper around a [`serde_json::Value`] whose `Serialize` impl
/// maps `Value::Null` to `serialize_none()` (CBOR null) instead of the
/// default `serialize_unit()` (CBOR empty array), recursively.
pub(super) struct EncodableValue<'a>(pub(super) &'a Value);

impl Serialize for EncodableValue<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self.0 {
            Value::Null => serializer.serialize_none(),
            Value::Bool(value) => serializer.serialize_bool(*value),
            Value::Number(number) => number.serialize(serializer),
            Value::String(string) => serializer.serialize_str(string),
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
    use serde_json::json;

    use super::*;

    #[test]
    fn top_level_null_encodes_as_cbor_null_not_empty_array() {
        let bytes = CborCodec
            .encode(&EncodableValue(&Value::Null))
            .expect("encode");
        assert_eq!(bytes, vec![0xf6]);
    }

    #[test]
    fn nested_null_encodes_as_cbor_null_and_round_trips() {
        let value = json!({"a": null, "b": [1, null, "x"], "c": null});
        let bytes = CborCodec.encode(&EncodableValue(&value)).expect("encode");
        let decoded: Value = CborCodec.decode(&bytes).expect("decode");
        assert_eq!(decoded, value);
    }

    #[test]
    fn non_null_values_round_trip_unchanged() {
        let value = json!({"name": "cratestack", "count": 3, "ok": true});
        let bytes = CborCodec.encode(&EncodableValue(&value)).expect("encode");
        let decoded: Value = CborCodec.decode(&bytes).expect("decode");
        assert_eq!(decoded, value);
    }
}
