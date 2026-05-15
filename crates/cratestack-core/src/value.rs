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
