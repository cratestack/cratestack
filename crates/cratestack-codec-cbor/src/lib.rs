use cratestack_core::{CratestackCodec, CratestackError};
use serde::{Deserialize, Serialize};

pub mod compact;
mod depth;
mod limited;
mod seq;

pub use depth::{DEFAULT_MAX_DECODE_DEPTH, check_depth};
pub use limited::{CborCodecWithDepth, decode_with_max_depth};
pub use seq::CborSeqChunkDecoder;

/// The CBOR codec. `decode` refuses input nested deeper than
/// [`DEFAULT_MAX_DECODE_DEPTH`] levels (cratestack#1315); use
/// [`CborCodec::with_max_depth`] for another limit.
#[derive(Debug, Clone, Default)]
pub struct CborCodec;

impl CborCodec {
    /// A codec whose `decode` accepts up to `max_depth` nested arrays, maps
    /// and tags (a scalar is depth 0). Encoding is unchanged.
    pub fn with_max_depth(max_depth: usize) -> CborCodecWithDepth {
        CborCodecWithDepth::new(max_depth)
    }
}

impl CratestackCodec for CborCodec {
    const CONTENT_TYPE: &'static str = "application/cbor";

    fn encode<T: Serialize + ?Sized>(&self, value: &T) -> Result<Vec<u8>, CratestackError> {
        // `minicbor-serde` reports `is_human_readable() == false` — verified
        // by encoding a probe type whose `Serialize` echoes the hint; it
        // emits `0xf4` (CBOR false). Types whose serde impl branches on that
        // hint (`uuid::Uuid`, `cratestack_core::Value`'s `Bytes` arm)
        // therefore take their binary branch here, which is the intended
        // behavior: `ProjectedValue` (cratestack#430) exists precisely to
        // defer that branch to this serializer rather than baking in
        // `serde_json`'s always-human-readable answer upstream.
        // `chrono::DateTime` has no such branch: its `Serialize` always
        // writes RFC 3339 text, so it is a text string here too: 21 to 32
        // bytes by fractional precision (`docs/design/enum-ordinals.md` §8).
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
        self.encode_into(value, &mut buf)?;
        Ok(buf)
    }

    /// The serializer writes straight into `out`, so a signing envelope's
    /// `seal_value` (cratestack#1005) gets the payload in its own buffer with
    /// no intermediate `Vec`. `encode` is this into an empty `Vec`, so the
    /// two cannot drift apart.
    fn encode_into<T: Serialize + ?Sized>(
        &self,
        value: &T,
        out: &mut Vec<u8>,
    ) -> Result<(), CratestackError> {
        let start = out.len();
        let result = {
            let mut serializer = minicbor_serde::Serializer::new(&mut *out);
            serializer.serialize_unit_as_null(true);
            value.serialize(&mut serializer)
        };
        result.map_err(|error| {
            // The trait asks an override to leave `out` as it found it.
            out.truncate(start);
            CratestackError::Codec(format!("failed to encode CBOR body: {error}"))
        })
    }

    /// Decodes a CBOR value that must consume all of `bytes`, wrapped in
    /// `serde_path_to_error` so the error's detail names the field that
    /// failed (`failed to decode CBOR body: amountE8: ...`). The public
    /// message of `CratestackError::Codec` stays generic; the path reaches
    /// operators through `detail()` only.
    ///
    /// Unconsumed bytes are an error (#1153), as with `JsonCodec::decode`'s
    /// trailing-characters check. `minicbor_serde::from_slice` would accept
    /// them silently. This also refuses a unit variant sent as a one-entry map
    /// (`{"A": null}`): minicbor-serde 0.7.1's `unit_variant` leaves the map
    /// value unconsumed. A first-party encoder sends unit variants as plain
    /// strings, so nothing legitimate is lost.
    ///
    /// `application/cbor-seq` does not rely on this method to split items: the
    /// buffered `cratestack-client-rust` `decode_cbor_sequence` walks items with
    /// its own deserializer, and the streaming decoders cut each item at
    /// `Decoder::skip()` boundaries before decoding the exact item bytes
    /// (Flutter's path then runs each item through this `decode`, where every
    /// slice is exactly one item).
    fn decode<T: for<'de> Deserialize<'de>>(&self, bytes: &[u8]) -> Result<T, CratestackError> {
        decode_with_max_depth(bytes, DEFAULT_MAX_DECODE_DEPTH)
    }
}

pub(crate) fn decode_error(error: impl std::fmt::Display) -> CratestackError {
    CratestackError::Codec(format!("failed to decode CBOR body: {error}"))
}

#[cfg(test)]
mod tests_trailing;

#[cfg(test)]
mod tests;
