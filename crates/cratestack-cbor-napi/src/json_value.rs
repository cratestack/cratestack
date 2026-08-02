//! JSON-to-CBOR serialization shim for the `encode` boundary.
//!
//! `cratestack-codec-cbor`'s `CborCodec` (which this crate wraps unchanged
//! — see the crate root docs) is generic over any `T: Serialize`. Feeding
//! it a `serde_json::Value` directly works for every JSON shape *except*
//! `null`: `Value::Null`'s `Serialize` impl calls `serializer.serialize_unit()`,
//! and `minicbor-serde`'s default `Serializer` renders a Rust unit `()` as
//! the CBOR empty-array marker (`0x80`), not the CBOR null marker (`0xf6`)
//! — the exact non-RFC-8949 quirk `CborCodec::encode`'s own doc comment
//! warns about. `CborCodec`'s own test suite only proves `Option::None`
//! round-trips correctly because `Option::None`'s `Serialize` impl calls
//! `serialize_none()`, a different serde method `minicbor-serde` maps to
//! CBOR null unconditionally.
//!
//! A napi `encode` entry point takes arbitrary JS values with no `Option`
//! wrapper to reach for — JS has exactly one "no value" concept (`null`),
//! surfacing as `serde_json::Value::Null` — so without this shim, every
//! JSON `null` in a payload (top-level or nested inside an object/array)
//! would silently round-trip as `[]` once decoded back. `CborValue`
//! re-routes `Value::Null` through `serialize_none` instead, recursively,
//! so it matches `CborCodec`'s own `Option::None` behavior byte-for-byte —
//! without touching `cratestack-codec-cbor` or `minicbor-serde` themselves.
//! This is JS/Rust boundary translation, not new CBOR wire-format logic:
//! every branch below just picks which existing serde method to call.

use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};
use serde_json::Value;

/// Borrowing wrapper around a [`serde_json::Value`] whose `Serialize` impl
/// maps `Value::Null` to `serialize_none()` (CBOR null) instead of the
/// default `serialize_unit()` (CBOR empty array). See module docs.
pub struct CborValue<'a>(pub &'a Value);

impl Serialize for CborValue<'_> {
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
                    seq.serialize_element(&CborValue(item))?;
                }
                seq.end()
            }
            Value::Object(entries) => {
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for (key, value) in entries {
                    map.serialize_entry(key, &CborValue(value))?;
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
    use serde_json::json;

    use super::*;

    #[test]
    fn top_level_null_encodes_as_cbor_null_not_empty_array() {
        // Same wire assertion as cratestack-codec-cbor's own
        // `optional_none_round_trips_as_cbor_null` test, proving this
        // wrapper preserves it for a `serde_json::Value` input.
        let bytes = CborCodec.encode(&CborValue(&Value::Null)).expect("encode");
        assert_eq!(bytes, vec![0xf6]);
    }

    #[test]
    fn nested_null_encodes_as_cbor_null_and_round_trips() {
        let value = json!({"a": null, "b": [1, null, "x"], "c": null});
        let bytes = CborCodec.encode(&CborValue(&value)).expect("encode");
        let decoded: Value = CborCodec.decode(&bytes).expect("decode");
        assert_eq!(decoded, value);
    }

    #[test]
    fn non_null_values_round_trip_unchanged() {
        let value = json!({"name": "cratestack", "count": 3, "ok": true});
        let bytes = CborCodec.encode(&CborValue(&value)).expect("encode");
        let decoded: Value = CborCodec.decode(&bytes).expect("decode");
        assert_eq!(decoded, value);
    }
}
