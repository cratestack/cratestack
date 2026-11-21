//! Outbound counterpart of [`crate::lenient_bytes`]: writes a schema
//! `Bytes` / `Geography` / `Geometry` value as a CBOR byte string (RFC 8949
//! major type 2) — cratestack#1425.
//!
//! These types generate as plain `Vec<u8>`, whose blanket `Serialize` is
//! serde's *sequence* form: under CBOR an array of integers (`83 01 02 18
//! ff`), roughly twice the size of the bytes it carries, and a generic
//! decoder materialises one value per byte. [`serialize_bytes_shape`] (and
//! [`WireBytes`], for values that bypass a struct's `Serialize`) call
//! `serialize_bytes` instead, **only when the serializer is not
//! human-readable**. A human-readable serializer — JSON bodies, SSE,
//! `serde_json::to_value` for audit/outbox/cursors/MCP — gets the value's
//! own `Serialize` unchanged, so JSON stays byte-identical.
//!
//! The helper is generic over [`BytesShape`] rather than one function per
//! field shape: serde hands `serialize_with` a `&FieldType`, so the single
//! generic function covers `Vec<u8>`, `Vec<Vec<u8>>` and any `Option` nesting
//! of them. `None` stays `serialize_none`, so a patch's `Some(None)` is still
//! `null`.

use serde::{Serialize, Serializer};

/// A value built from `Vec<u8>` leaves that can be written with each leaf as
/// a byte string where the format wants one. Implemented for `Vec<u8>`,
/// `Vec<Vec<u8>>`, `&T` and `Option<T>` of any of those.
pub trait BytesShape {
    /// Serialize `self`, writing each byte leaf as `serialize_bytes` when
    /// `serializer` is not human-readable.
    fn serialize_wire<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error>;
}

/// Whether a byte leaf should be written as a byte string for `serializer`.
fn writes_byte_strings<S: Serializer>(serializer: &S) -> bool {
    !serializer.is_human_readable()
}

impl BytesShape for Vec<u8> {
    fn serialize_wire<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if writes_byte_strings(&serializer) {
            serializer.serialize_bytes(self)
        } else {
            // Exactly what the derive emitted before cratestack#1425.
            Serialize::serialize(self, serializer)
        }
    }
}

impl BytesShape for Vec<Vec<u8>> {
    fn serialize_wire<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.iter().map(WireBytes))
    }
}

impl<T: BytesShape> BytesShape for Option<T> {
    fn serialize_wire<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            None => serializer.serialize_none(),
            Some(value) => serializer.serialize_some(&WireBytes(value)),
        }
    }
}

impl<T: BytesShape + ?Sized> BytesShape for &T {
    fn serialize_wire<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (**self).serialize_wire(serializer)
    }
}

/// Adapter whose `Serialize` is [`BytesShape::serialize_wire`]. Used where a
/// value is serialized without going through a generated struct — the
/// projection leaves — and internally for list elements and `Some` payloads.
pub struct WireBytes<T>(pub T);

impl<T: BytesShape> Serialize for WireBytes<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize_wire(serializer)
    }
}

/// The `#[serde(serialize_with = "…")]` target the macros attach to every
/// generated `Bytes`/`Geography`/`Geometry` field. Also usable on your own
/// `Vec<u8>` fields to get the same wire form.
pub fn serialize_bytes_shape<T: BytesShape, S: Serializer>(
    value: &T,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    value.serialize_wire(serializer)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_json;
