//! Backend-agnostic JSON-shaped value used throughout the framework
//! (auth claims, audit payloads, RPC error details, schema config).

#[cfg(test)]
mod tests;

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

impl Value {
    /// Convert to the **plain, untagged** JSON shape used for a schema
    /// `Json` column's on-disk representation (cratestack#162): an empty
    /// map becomes `{}`, a list becomes `[...]`, `Value::Null` becomes
    /// `null` — never `Value`'s own derived, externally-tagged wire
    /// format (`{"Map": {}}`), which stays reserved for the typed/wire
    /// contexts that need the exact variant back (auth claims, audit
    /// payloads, RPC error details).
    ///
    /// `Value::Bytes` has no native JSON representation, so it is
    /// base64-encoded into a JSON string. That direction is lossy on the
    /// way back: [`Value::from_plain_json`] has no way to tell a
    /// base64-looking string from an ordinary one, so it always decodes
    /// JSON strings as `Value::String`. Callers that need `Bytes` to
    /// round-trip losslessly should use a `Bytes` column, not `Json`.
    pub fn to_plain_json(&self) -> serde_json::Value {
        match self {
            Value::Null => serde_json::Value::Null,
            Value::Bool(value) => serde_json::Value::Bool(*value),
            Value::Int(value) => serde_json::Value::Number((*value).into()),
            Value::Float(value) => serde_json::Number::from_f64(*value)
                .map(serde_json::Value::Number)
                // NaN / +-infinity have no JSON representation; `Null` is
                // the least-surprising fallback (matches how `Option`
                // fields already collapse to SQL/JSON null elsewhere).
                .unwrap_or(serde_json::Value::Null),
            Value::String(value) => serde_json::Value::String(value.clone()),
            Value::Bytes(bytes) => {
                use base64::Engine;
                serde_json::Value::String(base64::engine::general_purpose::STANDARD.encode(bytes))
            }
            Value::List(items) => {
                serde_json::Value::Array(items.iter().map(Value::to_plain_json).collect())
            }
            Value::Map(map) => serde_json::Value::Object(
                map.iter()
                    .map(|(key, value)| (key.clone(), value.to_plain_json()))
                    .collect(),
            ),
        }
    }

    /// Inverse of [`Value::to_plain_json`]: parse a plain JSON value —
    /// cratestack's own past writes, legacy rows, or data written by any
    /// other JSON producer — into a `Value`. Every JSON number that fits
    /// in an `i64` decodes as `Value::Int`; everything else numeric
    /// decodes as `Value::Float`. Never produces `Value::Bytes` — see
    /// the round-trip caveat on [`Value::to_plain_json`].
    pub fn from_plain_json(json: serde_json::Value) -> Value {
        match json {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(value) => Value::Bool(value),
            serde_json::Value::Number(number) => match number.as_i64() {
                Some(value) => Value::Int(value),
                None => Value::Float(number.as_f64().unwrap_or_default()),
            },
            serde_json::Value::String(value) => Value::String(value),
            serde_json::Value::Array(items) => {
                Value::List(items.into_iter().map(Value::from_plain_json).collect())
            }
            serde_json::Value::Object(map) => Value::Map(
                map.into_iter()
                    .map(|(key, value)| (key, Value::from_plain_json(value)))
                    .collect(),
            ),
        }
    }
}
