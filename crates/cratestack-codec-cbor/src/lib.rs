use cratestack_core::{CratestackCodec, CratestackError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default)]
pub struct CborCodec;

impl CratestackCodec for CborCodec {
    const CONTENT_TYPE: &'static str = "application/cbor";

    fn encode<T: Serialize + ?Sized>(&self, value: &T) -> Result<Vec<u8>, CratestackError> {
        // `minicbor-serde` reports `is_human_readable() == false` — verified
        // by encoding a probe type whose `Serialize` echoes the hint; it
        // emits `0xf4` (CBOR false). Types whose serde impl branches on that
        // hint (`uuid::Uuid`, `chrono::DateTime`, `cratestack_core::Value`'s
        // `Bytes` arm) therefore take their binary branch here, which is the
        // intended behavior: `ProjectedValue` (cratestack#430) exists
        // precisely to defer that branch to this serializer rather than
        // baking in `serde_json`'s always-human-readable answer upstream.
        //
        // This backend does encode `()` as `0x80` (an empty array) rather
        // than RFC 8949 null. Nothing relies on the old workaround of
        // stripping `Value::Null` map entries before encoding — that was
        // removed in #430. Both `ProjectedValue::Null` and
        // `cratestack_core::Value::Null` call `serialize_none()`, which this
        // backend encodes correctly as `0xf6`.
        minicbor_serde::to_vec(value)
            .map_err(|error| CratestackError::Codec(format!("failed to encode CBOR body: {error}")))
    }

    fn decode<T: for<'de> Deserialize<'de>>(&self, bytes: &[u8]) -> Result<T, CratestackError> {
        minicbor_serde::from_slice(bytes)
            .map_err(|error| CratestackError::Codec(format!("failed to decode CBOR body: {error}")))
    }
}

#[cfg(test)]
mod tests {
    use cratestack_core::CratestackCodec;

    use super::CborCodec;

    #[test]
    fn round_trips_value() {
        let codec = CborCodec;
        let bytes = codec
            .encode(&vec!["cool", "stack"])
            .expect("encode should succeed");
        let value: Vec<String> = codec.decode(&bytes).expect("decode should succeed");

        assert_eq!(value, vec!["cool".to_owned(), "stack".to_owned()]);
    }

    #[test]
    fn optional_none_round_trips_as_cbor_null() {
        // minicbor-serde encodes `Option::<T>::None` as the CBOR null
        // marker (`0xf6`, RFC 8949 §3.3 simple-value 22) — which is what
        // we want. `serde_json::Value::Null` would mis-encode here as the
        // CBOR empty-array marker (`0x80`); the macro-emitted projection
        // strips `Value::Null` map entries *before* they reach this codec
        // so the bug can't land on the wire.
        let codec = CborCodec;
        let bytes = codec.encode(&Option::<String>::None).expect("encode none");
        assert_eq!(bytes, vec![0xf6]);
        let decoded: Option<String> = codec.decode(&bytes).expect("decode none");
        assert!(decoded.is_none());
    }
}
