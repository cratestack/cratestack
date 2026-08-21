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
        // By default `minicbor-serde` encodes `serialize_unit()` (bare `()`,
        // and anything whose `Serialize` impl calls it) as `0x80` — a CBOR
        // empty array — rather than RFC 8949 null `0xf6`. `Option::None` and
        // `cratestack_core::Value::Null`/`ProjectedValue::Null` never hit
        // that path (they call `serialize_none()`, which this backend
        // already encodes correctly as `0xf6`), but `serde_json::Value`'s
        // `Serialize` impl calls `serialize_unit()` for `Value::Null`. That
        // matters here because `POST /rpc/batch` carries every frame's
        // input/output as opaque `serde_json::Value` (the batch envelope
        // can't know each frame's real Rust type up front — see
        // `RpcRequest`/`RpcResponseFrame` in `cratestack-core::rpc`), so a
        // `null` anywhere in a batched frame used to round-trip through
        // this codec as `serialize_unit()`, landing on the wire as `0x80`
        // instead of `0xf6` (cratestack#657). `serialize_unit_as_null(true)`
        // makes `serialize_unit()` behave like `serialize_none()` for every
        // caller of this codec, closing that gap without requiring every
        // opaque-`Value` call site to route through `ProjectedValue` first.
        let mut buf = Vec::new();
        let mut serializer = minicbor_serde::Serializer::new(&mut buf);
        serializer.serialize_unit_as_null(true);
        value.serialize(&mut serializer).map_err(|error| {
            CratestackError::Codec(format!("failed to encode CBOR body: {error}"))
        })?;
        Ok(buf)
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
    fn repro_serde_json_null_mis_encodes_as_empty_array() {
        // cratestack#657 reproduction: `serde_json::Value::Null`'s
        // `Serialize` impl calls `serialize_unit()`, not
        // `serialize_none()`, so plain `minicbor_serde::to_vec` (no
        // `serialize_unit_as_null`) encodes it as the CBOR empty-array
        // marker `0x80`, not RFC 8949 null `0xf6`.
        let bytes = minicbor_serde::to_vec(&serde_json::Value::Null).expect("encode null");
        assert_eq!(bytes, vec![0x80]);
    }

    #[test]
    fn optional_none_round_trips_as_cbor_null() {
        // minicbor-serde encodes `Option::<T>::None` as the CBOR null
        // marker (`0xf6`, RFC 8949 §3.3 simple-value 22) — which is what
        // we want, via `serialize_none()`.
        let codec = CborCodec;
        let bytes = codec.encode(&Option::<String>::None).expect("encode none");
        assert_eq!(bytes, vec![0xf6]);
        let decoded: Option<String> = codec.decode(&bytes).expect("decode none");
        assert!(decoded.is_none());
    }

    #[test]
    fn json_null_round_trips_as_cbor_null_through_this_codec() {
        // cratestack#657 regression guard, scoped to this codec: unlike the
        // bare `minicbor_serde::to_vec` reproduction above,
        // `CborCodec::encode` enables `serialize_unit_as_null`, so
        // `serde_json::Value::Null` — which takes the `serialize_unit()`
        // path, not `serialize_none()` — now also encodes as `0xf6`. This
        // is the type the `/rpc/batch` envelope actually carries per frame
        // (`RpcRequest.input`/`RpcResponseFrame.output` are opaque
        // `serde_json::Value`, not `Option<T>`), so this is the case that
        // previously reached the wire as `0x80` on the batch path even
        // though `optional_none_round_trips_as_cbor_null` above was green.
        let codec = CborCodec;
        let bytes = codec
            .encode(&serde_json::Value::Null)
            .expect("encode json null");
        assert_eq!(bytes, vec![0xf6]);
        let decoded: serde_json::Value = codec.decode(&bytes).expect("decode json null");
        assert!(decoded.is_null());
    }
}
