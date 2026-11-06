//! Builders shared by the CBOR rewrite tests: a value written exactly as
//! built, and a small recursive table.

use cratestack_core::compact::{KeyNode, KeyStruct, KeyTables};
use serde::Serialize;
use serde::ser::{SerializeMap, SerializeSeq, Serializer};
use serde_json::Value;

#[derive(Clone)]
pub(super) enum Key {
    Name(&'static str),
    Int(u64),
}

/// A CBOR value written exactly as built: map entries in order, keys of
/// either kind.
#[derive(Clone)]
pub(super) enum T {
    Map(Vec<(Key, T)>),
    Arr(Vec<T>),
    V(Value),
}

impl Serialize for T {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            T::V(value) => value.serialize(s),
            T::Arr(items) => {
                let mut seq = s.serialize_seq(Some(items.len()))?;
                items.iter().try_for_each(|i| seq.serialize_element(i))?;
                seq.end()
            }
            T::Map(entries) => {
                let mut map = s.serialize_map(Some(entries.len()))?;
                for (key, value) in entries {
                    match key {
                        Key::Name(name) => map.serialize_entry(name, value)?,
                        Key::Int(n) => map.serialize_entry(n, value)?,
                    }
                }
                map.end()
            }
        }
    }
}

pub(super) fn cbor<S: Serialize>(value: &S) -> Vec<u8> {
    let mut out = Vec::new();
    let mut s = minicbor_serde::Serializer::new(&mut out);
    s.serialize_unit_as_null(true);
    value.serialize(&mut s).expect("test values encode");
    out
}

pub(super) fn n(name: &'static str, value: T) -> (Key, T) {
    (Key::Name(name), value)
}

pub(super) fn i(index: u64, value: T) -> (Key, T) {
    (Key::Int(index), value)
}

pub(super) fn v(value: Value) -> T {
    T::V(value)
}

/// `Order { id, total, customer: Customer?, lines: Line[], meta: Json }`,
/// `Customer { name, orders: Order[] }` (recursive), `Line { sku, qty }`.
pub(super) fn tables() -> KeyTables {
    let fields = |list: &[(&str, KeyNode)]| KeyStruct {
        fields: list
            .iter()
            .map(|(n, k)| ((*n).to_owned(), k.clone()))
            .collect(),
    };
    KeyTables {
        types: vec![
            fields(&[
                ("id", KeyNode::Opaque),
                ("total", KeyNode::Opaque),
                ("customer", KeyNode::Struct(1)),
                ("lines", KeyNode::List(Box::new(KeyNode::Struct(2)))),
                ("meta", KeyNode::Opaque),
            ]),
            fields(&[
                ("name", KeyNode::Opaque),
                ("orders", KeyNode::List(Box::new(KeyNode::Struct(0)))),
            ]),
            fields(&[("sku", KeyNode::Opaque), ("qty", KeyNode::Opaque)]),
        ],
        ops: vec![],
    }
}
