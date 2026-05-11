use cratestack_core::{CoolCodec, CoolError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default)]
pub struct CborCodec;

impl CoolCodec for CborCodec {
    const CONTENT_TYPE: &'static str = "application/cbor";

    fn encode<T: Serialize + ?Sized>(&self, value: &T) -> Result<Vec<u8>, CoolError> {
        // `minicbor-serde` reports `is_human_readable() = true`, which keeps
        // wire compatibility for types whose serde impl branches on that
        // hint (uuid, chrono::DateTime). The macro-emitted projection
        // strips `Value::Null` map entries before reaching this codec, so
        // the non-RFC-8949 "Null = empty array" quirk of this backend
        // never lands on the wire — see `project_*_model_value` in
        // cratestack-macros.
        minicbor_serde::to_vec(value)
            .map_err(|error| CoolError::Codec(format!("failed to encode CBOR body: {error}")))
    }

    fn decode<T: for<'de> Deserialize<'de>>(&self, bytes: &[u8]) -> Result<T, CoolError> {
        minicbor_serde::from_slice(bytes)
            .map_err(|error| CoolError::Codec(format!("failed to decode CBOR body: {error}")))
    }
}

#[cfg(test)]
mod tests {
    use cratestack_core::CoolCodec;

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
