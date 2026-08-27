//! Wire-shape-tolerant deserialization for schema `Bytes` fields — see
//! cratestack#783.
//!
//! A schema `Bytes` field generates as a plain `Vec<u8>`, and `Vec<u8>`'s
//! `Deserialize` is serde's blanket `Vec<T>` impl: a visitor with
//! `visit_seq` and nothing else. Over CBOR that accepts **only** an array
//! of integers (`0x84 01 02 03 04`) and hard-errors on a CBOR byte string
//! (`0x44 01020304`, RFC 8949 major type 2) with `unexpected type bytes …
//! expected array` — the shape every other CBOR producer emits for binary
//! data, and the shape `@cratestack/cbor` now writes for a JS
//! `Uint8Array`/`ArrayBuffer`.
//!
//! [`LenientBytes`] accepts both. `visit_bytes`/`visit_byte_buf` cover the
//! byte-string form, `visit_seq` keeps the integer-array form working
//! unchanged — which matters twice over: it is what every already-deployed
//! client (Rust, Dart, and any TypeScript caller doing the
//! `Array.from(bytes)` workaround) sends today, and it is the *only* shape
//! JSON can express, so the `application/json` transport is unaffected.
//!
//! **Inbound only.** Nothing here changes what a generated `Serialize`
//! emits: a `Bytes` field still goes out as an array of integers on both
//! transports and in all three client languages. Flipping the outbound
//! shape to a byte string is a genuine wire break for every existing
//! decoder (the Dart client's `cratestackAsValueList`, the TypeScript
//! client's `number[]`), so it is deliberately not bundled here.
//!
//! The `deserialize_*` wrappers exist one per generated field *shape*
//! rather than as a single generic function because `#[serde(deserialize_with
//! = "…")]` names a concrete function whose return type must match the
//! field's type exactly, and `crate::shared::bytes_serde` in
//! `cratestack-macros` picks between them from the field's arity and
//! whether it is patch-wrapped. See that module for the mapping.

use std::fmt;

use serde::Deserialize;
use serde::de::{Deserializer, Error, SeqAccess, Visitor};

/// `Vec<u8>` newtype whose `Deserialize` accepts a byte string *or* a
/// sequence of integers. See the module docs.
pub struct LenientBytes(pub Vec<u8>);

impl<'de> Deserialize<'de> for LenientBytes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // `deserialize_any`, not `deserialize_bytes`: both supported wire
        // formats are self-describing, and asking for bytes specifically
        // would make `minicbor-serde` demand a byte string and reject the
        // integer-array form every deployed client sends. Letting the
        // format say which visitor method to call is what makes accepting
        // both shapes possible at all — the same reasoning as
        // `crate::value::codec`'s `Deserialize for Value`.
        deserializer.deserialize_any(LenientBytesVisitor)
    }
}

struct LenientBytesVisitor;

impl<'de> Visitor<'de> for LenientBytesVisitor {
    type Value = LenientBytes;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a byte string or a sequence of byte-valued integers")
    }

    fn visit_bytes<E: Error>(self, value: &[u8]) -> Result<LenientBytes, E> {
        Ok(LenientBytes(value.to_vec()))
    }

    fn visit_byte_buf<E: Error>(self, value: Vec<u8>) -> Result<LenientBytes, E> {
        Ok(LenientBytes(value))
    }

    /// CBOR's definite-length text strings arrive here rather than at
    /// `visit_bytes`; a `Bytes` field has no string form on either
    /// transport, so this is left to the default `invalid_type` error.
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<LenientBytes, A::Error> {
        let mut bytes = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(byte) = seq.next_element::<u8>()? {
            bytes.push(byte);
        }
        Ok(LenientBytes(bytes))
    }
}

/// A required `Bytes` field — `Vec<u8>`.
pub fn deserialize_bytes<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
    LenientBytes::deserialize(deserializer).map(|bytes| bytes.0)
}

/// A nullable `Bytes` field, or a patch-wrapped required one —
/// `Option<Vec<u8>>`. Pair with `#[serde(default, …)]`: a custom
/// `deserialize_with` opts the field out of serde-derive's implicit
/// "missing `Option<T>` field defaults to `None`" (see [`crate::patch`]).
pub fn deserialize_optional_bytes<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<u8>>, D::Error> {
    Ok(Option::<LenientBytes>::deserialize(deserializer)?.map(|bytes| bytes.0))
}

/// A list-arity `Bytes` field — `Vec<Vec<u8>>`. Each element
/// independently accepts either shape.
pub fn deserialize_bytes_list<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Vec<u8>>, D::Error> {
    Ok(Vec::<LenientBytes>::deserialize(deserializer)?
        .into_iter()
        .map(|bytes| bytes.0)
        .collect())
}

/// A patch-wrapped list-arity `Bytes` field — `Option<Vec<Vec<u8>>>`.
/// Pair with `#[serde(default, …)]`, per [`deserialize_optional_bytes`].
pub fn deserialize_optional_bytes_list<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<Vec<u8>>>, D::Error> {
    Ok(Option::<Vec<LenientBytes>>::deserialize(deserializer)?
        .map(|list| list.into_iter().map(|bytes| bytes.0).collect()))
}

/// A patch-wrapped nullable `Bytes` field — `Option<Option<Vec<u8>>>`.
/// The `Bytes` counterpart of [`crate::patch::deserialize_double_option`]
/// (which can't be reused: its `T: Deserialize` bound resolves to
/// `Vec<u8>`'s strict blanket impl, the exact thing this module works
/// around). Same contract — the outer `Some` records "this key was
/// present", so it must be paired with `#[serde(default, …)]`.
pub fn deserialize_double_option_bytes<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<Vec<u8>>>, D::Error> {
    Ok(Some(
        Option::<LenientBytes>::deserialize(deserializer)?.map(|bytes| bytes.0),
    ))
}

#[cfg(test)]
mod tests;
