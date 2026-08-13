//! Hand-written, **untagged** `Serialize`/`Deserialize` for [`Value`].
//!
//! `Value` used to derive both, which emits serde's externally-tagged enum
//! representation: `Value::String("foo")` went on the wire as
//! `{"String":"foo"}` rather than `"foo"`, and `Value::Map(BTreeMap::new())`
//! as `{"Map":{}}` rather than `{}`. cratestack#162 / #395 fixed that for a
//! schema `Json` **column** by routing persistence through
//! [`Value::to_plain_json`], but every other path — procedure arguments and
//! results typed `Json`, auth claims, audit payloads, RPC error details —
//! still carried the tag. Consumers had to hand-write the tag at every call
//! site, and every generated Dart/TypeScript client inherited it.
//!
//! These impls make the *wire* shape match the persisted shape: plain,
//! self-describing, indistinguishable from what any other JSON or CBOR
//! producer would emit.
//!
//! ## Two format-specific details, both measured rather than assumed
//!
//! **`Null` serializes via `serialize_none`, never `serialize_unit`.** The
//! first-party CBOR backend (`minicbor-serde`, see `cratestack-codec-cbor`)
//! encodes `()` as `0x80` — an empty *array*, not RFC 8949 null — while
//! `None` correctly encodes as `0xf6`. Using `serialize_unit` here would put
//! that non-conformant shape on the wire for any `Value::Null` nested inside
//! a list or sent as a bare procedure argument.
//!
//! **`Bytes` branches on `is_human_readable()`.** Binary formats get a native
//! byte string (`serialize_bytes` → CBOR `0x44 de ad be ef`), which round-trips
//! losslessly. Human-readable formats have no byte type, so they get the same
//! base64 string [`Value::to_plain_json`] already writes — and inherit the same
//! documented asymmetry: a JSON string always decodes back as `Value::String`,
//! because nothing distinguishes base64 from ordinary text. Callers needing
//! `Bytes` to survive a JSON round-trip should use a `Bytes` column, not `Json`.
//!
//! Note that `minicbor-serde` reports `is_human_readable() == false`, so the
//! CBOR branch is the one that actually fires on the wire.

use std::collections::BTreeMap;
use std::fmt;

use base64::Engine;
use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::Value;

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            // `serialize_none`, not `serialize_unit` — see the module docs.
            Value::Null => serializer.serialize_none(),
            Value::Bool(value) => serializer.serialize_bool(*value),
            Value::Int(value) => serializer.serialize_i64(*value),
            Value::Float(value) => serializer.serialize_f64(*value),
            Value::String(value) => serializer.serialize_str(value),
            Value::Bytes(bytes) => {
                if serializer.is_human_readable() {
                    serializer
                        .serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
                } else {
                    serializer.serialize_bytes(bytes)
                }
            }
            Value::List(items) => serializer.collect_seq(items),
            Value::Map(map) => serializer.collect_map(map),
        }
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Value, D::Error> {
        // Both supported wire formats (JSON, CBOR) are self-describing, so
        // `deserialize_any` is the right entry point: the format tells us
        // which visitor method to call.
        deserializer.deserialize_any(ValueVisitor)
    }
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any JSON-shaped value")
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(ValueVisitor)
    }

    fn visit_bool<E>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Value, E> {
        Ok(Value::Int(value))
    }

    /// CBOR encodes small non-negative integers as unsigned, so an `Int`
    /// written by this codec can come back through `visit_u64`. Anything
    /// past `i64::MAX` has no `Value::Int` representation and degrades to
    /// `Float` rather than erroring — the same lossy-but-total policy
    /// `from_plain_json` applies to oversized JSON numbers.
    fn visit_u64<E>(self, value: u64) -> Result<Value, E> {
        Ok(match i64::try_from(value) {
            Ok(value) => Value::Int(value),
            Err(_) => Value::Float(value as f64),
        })
    }

    fn visit_i128<E>(self, value: i128) -> Result<Value, E> {
        Ok(match i64::try_from(value) {
            Ok(value) => Value::Int(value),
            Err(_) => Value::Float(value as f64),
        })
    }

    fn visit_u128<E>(self, value: u128) -> Result<Value, E> {
        Ok(match i64::try_from(value) {
            Ok(value) => Value::Int(value),
            Err(_) => Value::Float(value as f64),
        })
    }

    fn visit_f64<E>(self, value: f64) -> Result<Value, E> {
        Ok(Value::Float(value))
    }

    fn visit_str<E>(self, value: &str) -> Result<Value, E> {
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Value, E> {
        Ok(Value::String(value))
    }

    fn visit_bytes<E>(self, value: &[u8]) -> Result<Value, E> {
        Ok(Value::Bytes(value.to_vec()))
    }

    fn visit_byte_buf<E>(self, value: Vec<u8>) -> Result<Value, E> {
        Ok(Value::Bytes(value))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut items = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(Value::List(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Value, A::Error> {
        let mut map = BTreeMap::new();
        while let Some((key, value)) = access.next_entry::<String, Value>()? {
            map.insert(key, value);
        }
        Ok(Value::Map(map))
    }
}
