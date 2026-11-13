//! [`RpcFrameValue`]: the codec-neutral payload of a batch or subscription
//! frame (cratestack#1352, cratestack#1360).
//!
//! A frame's `input` / `output` / `next` used to be a `serde_json::Value`, which
//! has no byte-string arm: a CBOR `Uuid` (16-byte string) or `Bytes` could not
//! be decoded into it, so the whole `POST /rpc/batch` was refused (and a
//! subscription item could only be a `serde_json::to_value`, never a byte string). This type is
//! a `serde_json::Value` plus a [`RpcFrameValue::Bytes`] arm. It is serde-only
//! (no new dependency, builds for `wasm32`) and codec-neutral, so it also works
//! with custom codecs.
//!
//! For anything JSON can represent it serialises exactly as `serde_json::Value`
//! does (objects keep their `BTreeMap` order, numbers are `serde_json::Number`),
//! so JSON and CBOR bytes on the wire are unchanged.

use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};

use crate::codec::CratestackCodec;
use crate::error::CratestackError;

mod de;
#[cfg(test)]
mod tests;

/// A lossless, codec-neutral value for a batch or subscription frame's payload.
#[derive(Debug, Clone, PartialEq)]
pub enum RpcFrameValue {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    /// A byte string (CBOR major type 2). Under JSON, serde writes it as an
    /// array of numbers, like `serde_json::to_value` of a `serialize_bytes`.
    Bytes(Vec<u8>),
    Array(Vec<RpcFrameValue>),
    Object(BTreeMap<String, RpcFrameValue>),
}

impl RpcFrameValue {
    /// Encode `value` with `codec` and decode the bytes back into a frame
    /// value, keeping whatever the codec writes natively (byte strings).
    pub fn encode_with<C: CratestackCodec, T: Serialize + ?Sized>(
        codec: &C,
        value: &T,
    ) -> Result<Self, CratestackError> {
        codec.decode(&codec.encode(value)?)
    }

    /// The inverse of [`encode_with`](Self::encode_with): decode this value as
    /// a `T` the way `codec` would decode it off the wire.
    pub fn decode_with<C: CratestackCodec, T: DeserializeOwned>(
        &self,
        codec: &C,
    ) -> Result<T, CratestackError> {
        codec.decode(&codec.encode(self)?)
    }
}

impl Serialize for RpcFrameValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            // `serialize_unit`, not `serialize_none`: byte-identical to the
            // `serde_json::Value::Null` this type replaced.
            Self::Null => serializer.serialize_unit(),
            Self::Bool(b) => serializer.serialize_bool(*b),
            Self::Number(n) => n.serialize(serializer),
            Self::String(s) => serializer.serialize_str(s),
            Self::Bytes(b) => serializer.serialize_bytes(b),
            Self::Array(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            Self::Object(map) => {
                let mut out = serializer.serialize_map(Some(map.len()))?;
                for (key, value) in map {
                    out.serialize_entry(key, value)?;
                }
                out.end()
            }
        }
    }
}

impl From<serde_json::Value> for RpcFrameValue {
    fn from(value: serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => Self::Null,
            serde_json::Value::Bool(b) => Self::Bool(b),
            serde_json::Value::Number(n) => Self::Number(n),
            serde_json::Value::String(s) => Self::String(s),
            serde_json::Value::Array(a) => Self::Array(a.into_iter().map(Self::from).collect()),
            serde_json::Value::Object(o) => {
                Self::Object(o.into_iter().map(|(k, v)| (k, Self::from(v))).collect())
            }
        }
    }
}

impl From<RpcFrameValue> for serde_json::Value {
    /// `Bytes` becomes an array of numbers, matching `serde_json::to_value`.
    fn from(value: RpcFrameValue) -> Self {
        match value {
            RpcFrameValue::Null => Self::Null,
            RpcFrameValue::Bool(b) => Self::Bool(b),
            RpcFrameValue::Number(n) => Self::Number(n),
            RpcFrameValue::String(s) => Self::String(s),
            RpcFrameValue::Bytes(b) => Self::Array(b.into_iter().map(Self::from).collect()),
            RpcFrameValue::Array(a) => Self::Array(a.into_iter().map(Self::from).collect()),
            RpcFrameValue::Object(o) => {
                Self::Object(o.into_iter().map(|(k, v)| (k, Self::from(v))).collect())
            }
        }
    }
}
