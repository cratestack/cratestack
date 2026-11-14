//! Wire-shape-tolerant deserialization for schema `Uuid` fields — see
//! cratestack#1359.
//!
//! Over CBOR the server writes a `Uuid` as a 16-byte byte string (the
//! `uuid` crate's non-human-readable serde form, RFC 8949 major type 2),
//! and `uuid::Uuid`'s own `Deserialize` accepts *only* that form there. A
//! TypeScript or Dart client holds a `Uuid` as a plain string and cannot
//! tell it apart from any other string at encode time (the Dart bridge
//! carries JSON text, which has no byte-string primitive at all), so it
//! sends the text form — which the strict decode refused.
//!
//! [`LenientUuid`] accepts both under a non-human-readable format: a text
//! string (parsed) or a byte string (16 bytes). Under a human-readable
//! format (JSON) it delegates to `uuid::Uuid` unchanged, so the JSON wire,
//! its accepted shapes and its error text are byte-for-byte what they were.
//!
//! **Inbound only.** A `Uuid` still goes out as 16 bytes under CBOR; the
//! generated TypeScript and Dart clients revive that into the canonical
//! string on decode. The `deserialize_*` wrappers exist one per generated
//! field *shape*, exactly as in [`crate::lenient_bytes`], because
//! `#[serde(deserialize_with)]` names a concrete function whose return
//! type must match the field type. `crate::shared::bytes_serde` in
//! `cratestack-macros` maps a field's arity to the right one.

use std::fmt;

use serde::Deserialize;
use serde::de::{Deserializer, Error, Visitor};
use uuid::Uuid;

use crate::FieldFilterInput;

/// `Uuid` newtype whose `Deserialize` accepts text or 16 bytes under a
/// binary format. See the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LenientUuid(pub Uuid);

impl From<LenientUuid> for Uuid {
    fn from(value: LenientUuid) -> Self {
        value.0
    }
}

impl<'de> Deserialize<'de> for LenientUuid {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            // JSON path: keep `uuid`'s own behaviour and error text.
            return Uuid::deserialize(deserializer).map(LenientUuid);
        }
        // `deserialize_any`: the binary format (CBOR) is self-describing,
        // so it can say whether this is a text or a byte string.
        deserializer.deserialize_any(LenientUuidVisitor)
    }
}

struct LenientUuidVisitor;

impl<'de> Visitor<'de> for LenientUuidVisitor {
    type Value = LenientUuid;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a UUID string or 16 bytes")
    }

    fn visit_str<E: Error>(self, value: &str) -> Result<LenientUuid, E> {
        value
            .parse::<Uuid>()
            .map(LenientUuid)
            .map_err(|e| E::custom(format_args!("UUID parsing failed: {e}")))
    }

    fn visit_bytes<E: Error>(self, value: &[u8]) -> Result<LenientUuid, E> {
        Uuid::from_slice(value)
            .map(LenientUuid)
            .map_err(|e| E::custom(format_args!("UUID parsing failed: {e}")))
    }
}

/// A required `Uuid` field.
pub fn deserialize_uuid<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Uuid, D::Error> {
    LenientUuid::deserialize(deserializer).map(|u| u.0)
}

/// A nullable `Uuid` field, or a patch-wrapped required one. Pair with
/// `#[serde(default, …)]` (see [`crate::patch`]).
pub fn deserialize_optional_uuid<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Uuid>, D::Error> {
    Ok(Option::<LenientUuid>::deserialize(deserializer)?.map(|u| u.0))
}

/// A list-arity `Uuid` field. Each element accepts either shape.
pub fn deserialize_uuid_list<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Uuid>, D::Error> {
    Ok(Vec::<LenientUuid>::deserialize(deserializer)?
        .into_iter()
        .map(|u| u.0)
        .collect())
}

/// A patch-wrapped list-arity `Uuid` field. Pair with `#[serde(default, …)]`.
pub fn deserialize_optional_uuid_list<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<Uuid>>, D::Error> {
    Ok(Option::<Vec<LenientUuid>>::deserialize(deserializer)?
        .map(|list| list.into_iter().map(|u| u.0).collect()))
}

/// A patch-wrapped nullable `Uuid` field — `Option<Option<Uuid>>`. The
/// outer `Some` records "this key was present", so pair with
/// `#[serde(default, …)]`; see [`crate::patch::deserialize_double_option`].
pub fn deserialize_double_option_uuid<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<Uuid>>, D::Error> {
    Ok(Some(
        Option::<LenientUuid>::deserialize(deserializer)?.map(|u| u.0),
    ))
}

/// A `<Model>Where` operator envelope over a `Uuid` column: every operand
/// (`eq`, `ne`, `in`, `lt`, …) accepts either shape. Pair with
/// `#[serde(default, …)]`.
pub fn deserialize_optional_uuid_filter<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<FieldFilterInput<Uuid>>, D::Error> {
    Ok(Option::<FieldFilterInput<LenientUuid>>::deserialize(deserializer)?
        .map(|filter| filter.map_values(|u| u.0)))
}

#[cfg(test)]
mod tests;
