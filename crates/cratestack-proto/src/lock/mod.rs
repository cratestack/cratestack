//! The lock's data model: [`PbLock`], [`MessageLock`], [`EnumLock`], and
//! their TOML round-trip. The TOML shape is normative — see
//! `docs/design/protobuf.md` §3.3 — and is matched field-for-field here so
//! a hand-edited lock (e.g. resolving the documented merge-conflict cost)
//! parses back the same way it was written.

mod assign;
mod build;
mod check;
mod numbering;
mod pin;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use build::build_lock;
pub use check::lock_would_change;

/// Numbers `19000..=19999` are reserved by protobuf itself and must never
/// be handed out by the auto-assign walker (rejected earlier, at `@pb`
/// parse time in `cratestack-parser`, for explicit pins).
pub(crate) const PROTO_RESERVED_RANGE: std::ops::RangeInclusive<i32> = 19000..=19999;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PbLock {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(default)]
    pub messages: BTreeMap<String, MessageLock>,
    #[serde(default)]
    pub enums: BTreeMap<String, EnumLock>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MessageLock {
    #[serde(flatten)]
    pub fields: BTreeMap<String, i32>,
    #[serde(default)]
    pub reserved: Vec<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EnumLock {
    #[serde(flatten)]
    pub variants: BTreeMap<String, i32>,
    #[serde(default)]
    pub reserved: Vec<i32>,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PbLockError {
    #[error(
        "`@pb({number})` on `{owner}.{field}` collides with a field number already in use on `{owner}`"
    )]
    PinCollidesWithUsed {
        owner: String,
        field: String,
        number: i32,
    },
    #[error(
        "`@pb({number})` on `{owner}.{field}` collides with a number already reserved on `{owner}`; \
         reserved numbers are never reassigned, even to the field that originally held one"
    )]
    PinCollidesWithReserved {
        owner: String,
        field: String,
        number: i32,
    },
    #[error("`{owner}.{field}` has an invalid `@pb` attribute `{raw}`: {reason}")]
    InvalidPin {
        owner: String,
        field: String,
        raw: String,
        reason: String,
    },
    #[error("failed to parse pb lock TOML: {0}")]
    Toml(#[from] toml::de::Error),
}

impl PbLock {
    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("PbLock only contains TOML-representable values")
    }

    pub fn from_toml(source: &str) -> Result<Self, PbLockError> {
        toml::from_str(source).map_err(PbLockError::from)
    }
}
