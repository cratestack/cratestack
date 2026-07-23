//! Backend-agnostic JSON-shaped value used throughout the framework
//! (auth claims, audit payloads, RPC error details, schema config).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use base64::Engine;

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
    /// Convert this Value into a `serde_json::Value` for storage as plain JSON.
    #[must_use]
    pub fn into_json(self) -> serde_json::Value {
        use serde_json::{Number as JsonNumber, Value as JsonVal};
        match self {
            Value::Null => JsonVal::Null,
            Value::Bool(b) => JsonVal::Bool(b),
            Value::Int(i) => JsonVal::Number(JsonNumber::from(i)),
            Value::Float(f) => {
               if f.is_finite() {
                   JsonVal::Number(JsonNumber::from_f64(f).expect("finite floats convert to JsonNumber"))
               } else {
                   // Represent non-finite floats as a tagged object so they
                   // round-trip through JSON (NaN / Infinity are not valid
                   // JSON numbers).
                   let mut obj = serde_json::Map::new();
                   let tag = if f.is_nan() {
                       "NaN"
                   } else if f.is_sign_positive() {
                       "Infinity"
                   } else {
                       "-Infinity"
                   };
                   obj.insert("__Float".to_string(), JsonVal::String(tag.to_string()));
                   JsonVal::Object(obj)
               }
           }
           Value::String(s) => JsonVal::String(s),
           Value::Bytes(b) => {
               // Encode bytes as a tagged base64 string so they round-trip
               // through JSON while staying unambiguous from regular
               // string content.
               let mut obj = serde_json::Map::new();
               obj.insert(
                   "__Bytes".to_string(),
                   JsonVal::String(base64::engine::general_purpose::STANDARD.encode(&b)),
               );
               JsonVal::Object(obj)
           }
           // use direct method reference instead of closure for pedantic clippy
           Value::List(vec) => JsonVal::Array(vec.into_iter().map(Value::into_json).collect()),
           Value::Map(map) => {
               let mut obj = serde_json::Map::new();
               for (k, v) in map {
                   obj.insert(k, v.into_json());
               }
               JsonVal::Object(obj)
           }
       }
    }
 
    /// Convert from a `serde_json::Value` (plain JSON) into the internal `Value`.
    #[must_use]
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
                        // Converting large u64 to f64 can lose precision; this
                        // fallback is rare and documented. Silence the pedantic
                        // lint for this cast only.
                        #[allow(clippy::cast_precision_loss)]
                        {
                            Value::Float(u as f64)
                        }
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
                // Detect tagged forms we emit for Bytes and non-finite floats.
                if obj.len() == 1 {
                    if let Some(v) = obj.get("__Bytes") {
                        if let JsonVal::String(s) = v {
                            let decoded = base64::engine::general_purpose::STANDARD
                                .decode(s)
                                .unwrap_or_default();
                            return Value::Bytes(decoded);
                        }
                    }
                    if let Some(v) = obj.get("__Float") {
                        if let JsonVal::String(tag) = v {
                            match tag.as_str() {
                                "NaN" => return Value::Float(f64::NAN),
                                "Infinity" => return Value::Float(f64::INFINITY),
                                "-Infinity" => return Value::Float(f64::NEG_INFINITY),
                                _ => {}
                            }
                        }
                    }
                }
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
