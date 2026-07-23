//! Backend-agnostic JSON-shaped value used throughout the framework
//! (auth claims, audit payloads, RPC error details, schema config).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    List(Vec<Value>),
    Map(BTreeMap<String, Value>),
}

impl Value {
    /// Convert this Value into a serde_json::Value for storage as plain JSON.
    pub fn into_json(self) -> serde_json::Value {
        use serde_json::{Number as JsonNumber, Value as JsonVal};
        match self {
            Value::Null => JsonVal::Null,
            Value::Bool(b) => JsonVal::Bool(b),
            Value::Int(i) => JsonVal::Number(JsonNumber::from(i)),
            Value::Float(f) => JsonVal::Number(JsonNumber::from_f64(f).unwrap_or(JsonNumber::from(0))),
            Value::String(s) => JsonVal::String(s),
            Value::Bytes(b) => JsonVal::Array(b.into_iter().map(|byte| JsonVal::Number(JsonNumber::from(byte))).collect()),
            Value::List(vec) => JsonVal::Array(vec.into_iter().map(|v| v.into_json()).collect()),
            Value::Map(map) => {
                let mut obj = serde_json::Map::new();
                for (k, v) in map {
                    obj.insert(k, v.into_json());
                }
                JsonVal::Object(obj)
            }
        }
    }

    /// Convert from a serde_json::Value (plain JSON) into the internal Value.
    pub fn from_json(v: serde_json::Value) -> Self {
        use serde_json::Value as JsonVal;
        match v {
            JsonVal::Null => Value::Null,
            JsonVal::Bool(b) => Value::Bool(b),
            JsonVal::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Value::Int(i)
                } else if let Some(f) = n.as_f64() {
                    Value::Float(f)
                } else if let Some(u) = n.as_u64() {
                    // best-effort: fit unsigned into i64 when possible
                    if let Ok(i) = i64::try_from(u) {
                        Value::Int(i)
                    } else {
                        Value::Float(u as f64)
                    }
                } else {
                    // fallback
                    Value::Float(n.as_f64().unwrap_or(0.0))
                }
            }
            JsonVal::String(s) => Value::String(s),
            JsonVal::Array(arr) => {
                // arrays might represent bytes or lists; treat as List
                Value::List(arr.into_iter().map(Value::from_json).collect())
            }
            JsonVal::Object(obj) => {
                let mut map = std::collections::BTreeMap::new();
                for (k, v) in obj {
                    map.insert(k, Value::from_json(v));
                }
                Value::Map(map)
            }
        }
    }
}

/// `Default` is required by every generated model struct since #51
/// (column projection): non-selected fields hold `T::default()` so the
/// returned `Projection<T>` is constructable without re-fetching.
/// `Value::Null` is the natural identity — JSON columns surfacing as
/// `cratestack::Value` default to "no payload" until the next read.
impl Default for Value {
    fn default() -> Self {
        Value::Null
    }
}
