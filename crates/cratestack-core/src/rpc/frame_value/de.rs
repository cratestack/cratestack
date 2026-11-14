//! `Deserialize` for [`RpcFrameValue`]: serde_json's `Value` visitor plus the
//! byte-string arms it lacks.

use std::collections::BTreeMap;
use std::fmt;

use serde::de::value::{I128Deserializer, U128Deserializer};
use serde::de::{Error, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};

use super::RpcFrameValue;

struct FrameValueVisitor;

impl<'de> Deserialize<'de> for RpcFrameValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(FrameValueVisitor)
    }
}

impl<'de> Visitor<'de> for FrameValueVisitor {
    type Value = RpcFrameValue;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("any valid batch frame value")
    }

    fn visit_bool<E>(self, v: bool) -> Result<RpcFrameValue, E> {
        Ok(RpcFrameValue::Bool(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<RpcFrameValue, E> {
        Ok(RpcFrameValue::Number(v.into()))
    }

    fn visit_u64<E>(self, v: u64) -> Result<RpcFrameValue, E> {
        Ok(RpcFrameValue::Number(v.into()))
    }

    fn visit_i128<E: Error>(self, v: i128) -> Result<RpcFrameValue, E> {
        serde_json::Number::deserialize(I128Deserializer::<E>::new(v)).map(RpcFrameValue::Number)
    }

    fn visit_u128<E: Error>(self, v: u128) -> Result<RpcFrameValue, E> {
        serde_json::Number::deserialize(U128Deserializer::<E>::new(v)).map(RpcFrameValue::Number)
    }

    fn visit_f64<E>(self, v: f64) -> Result<RpcFrameValue, E> {
        Ok(serde_json::Number::from_f64(v).map_or(RpcFrameValue::Null, RpcFrameValue::Number))
    }

    fn visit_str<E>(self, v: &str) -> Result<RpcFrameValue, E> {
        Ok(RpcFrameValue::String(v.to_owned()))
    }

    fn visit_string<E>(self, v: String) -> Result<RpcFrameValue, E> {
        Ok(RpcFrameValue::String(v))
    }

    fn visit_bytes<E>(self, v: &[u8]) -> Result<RpcFrameValue, E> {
        Ok(RpcFrameValue::Bytes(v.to_vec()))
    }

    fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<RpcFrameValue, E> {
        Ok(RpcFrameValue::Bytes(v))
    }

    fn visit_none<E>(self) -> Result<RpcFrameValue, E> {
        Ok(RpcFrameValue::Null)
    }

    fn visit_unit<E>(self) -> Result<RpcFrameValue, E> {
        Ok(RpcFrameValue::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<RpcFrameValue, D::Error> {
        RpcFrameValue::deserialize(d)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<RpcFrameValue, A::Error> {
        let mut items = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(4096));
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(RpcFrameValue::Array(items))
    }

    /// A repeated key keeps the last value, as `serde_json::Value` does.
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<RpcFrameValue, A::Error> {
        let mut out = BTreeMap::new();
        while let Some((key, value)) = map.next_entry::<String, RpcFrameValue>()? {
            out.insert(key, value);
        }
        Ok(RpcFrameValue::Object(out))
    }
}
