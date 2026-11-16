//! The codec a subscribe request's binding speaks, chosen at runtime.
//!
//! A dispatcher is generic over [`CratestackCodec`], but the binding is only
//! known once `Accept` has been read. This enum is the one codec the four
//! generated handlers hand every dispatcher, so SSE keeps its JSON items and
//! `application/cbor-seq` gets CBOR ones (a `Uuid` as a byte string, a
//! `Decimal` as its CBOR shape) without a `match` per handler (cratestack#1001).

use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use cratestack_core::{CratestackCodec, CratestackError};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// The JSON codec of SSE, or the CBOR codec of `application/cbor-seq`.
#[derive(Debug, Clone)]
pub enum SubscribeCodec {
    Json(JsonCodec),
    Cbor(CborCodec),
}

impl CratestackCodec for SubscribeCodec {
    /// Only the unary/batch paths read this; no subscribe path does, and the
    /// JSON codec's is the SSE binding's media type family.
    const CONTENT_TYPE: &'static str = JsonCodec::CONTENT_TYPE;

    fn encode<T: Serialize + ?Sized>(&self, value: &T) -> Result<Vec<u8>, CratestackError> {
        match self {
            Self::Json(codec) => codec.encode(value),
            Self::Cbor(codec) => codec.encode(value),
        }
    }

    fn encode_into<T: Serialize + ?Sized>(
        &self,
        value: &T,
        out: &mut Vec<u8>,
    ) -> Result<(), CratestackError> {
        match self {
            Self::Json(codec) => codec.encode_into(value, out),
            Self::Cbor(codec) => codec.encode_into(value, out),
        }
    }

    fn decode<T: DeserializeOwned>(&self, bytes: &[u8]) -> Result<T, CratestackError> {
        match self {
            Self::Json(codec) => codec.decode(bytes),
            Self::Cbor(codec) => codec.decode(bytes),
        }
    }
}
