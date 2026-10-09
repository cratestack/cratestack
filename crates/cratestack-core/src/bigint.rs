//! `BigInt`, the schema's 64-bit signed integer scalar (ADR 0019).
//!
//! It is a `Copy` newtype over `i64`, **not** an arbitrary-precision integer;
//! the name is the schema type's (`totalE8 BigInt`) and is namespaced as
//! `cratestack::BigInt`, so it does not collide with `num_bigint::BigInt`
//! unless one file imports both.
//!
//! # Wire form
//!
//! On every codec the value is a **canonical decimal string**: `0`, or an
//! optional `-`, a non-zero digit and at most 18 more digits, inside `i64`
//! (JSON Schema pattern `^(0|-?[1-9][0-9]{0,18})$`). In JSON that is a JSON
//! string; in CBOR it is a text string (major type 3), never an integer or a
//! bignum tag. One form on every codec means a value that crosses
//! `serde_json::Value` (the `/rpc/batch` frames) and then CBOR keeps the same
//! bytes, and no JavaScript number ever holds it.
//!
//! [`Serialize`](serde::Serialize) writes that string. [`Deserialize`](serde::Deserialize)
//! accepts nothing else: a JSON number, a CBOR integer, a CBOR tag 2 or 3,
//! `+5`, `007`, `-0`, `" 1"` and anything outside `i64` are all refused,
//! because a number above 2^53 may already have been rounded by the
//! JavaScript that produced it and the server cannot tell.
//!
//! # Arithmetic
//!
//! Checked methods only ([`BigInt::checked_add`], [`BigInt::checked_sub`],
//! [`BigInt::checked_mul`]). There are no operator impls, because an operator
//! on a 64-bit money value must panic, wrap or invent an error path, and no
//! `Deref`, because with one a call site nobody updated would keep compiling
//! against `i64` methods. A missed call site is a compile error.
//!
//! # Database support
//!
//! The `sqlx-postgres` and `rusqlite` Cargo features add the driver trait
//! impls (`Type`, `Encode`, `Decode`, `PgHasArrayType`; `ToSql`, `FromSql`) so
//! a `BigInt` primary or foreign key can be bound and read directly. Each
//! delegates to `i64` (`INT8` / SQLite integer). Neither is on by default and
//! `cratestack-core` takes no non-optional dependency on either driver; the
//! backend runtimes enable the one they need.

mod canonical;
mod serde_impl;

#[cfg(feature = "rusqlite")]
mod rusqlite_impl;
#[cfg(feature = "sqlx-postgres")]
mod sqlx_impl;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_cbor;
#[cfg(test)]
mod tests_json;
#[cfg(all(test, feature = "rusqlite"))]
mod tests_rusqlite;
#[cfg(all(test, feature = "sqlx-postgres"))]
mod tests_sqlx;

use core::fmt;
use core::str::FromStr;

pub use canonical::ParseBigIntError;

/// A signed 64-bit integer that travels as a canonical decimal string on every
/// codec. See the [module documentation](self) for the wire form.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BigInt(i64);

impl BigInt {
    /// The smallest value, `-9223372036854775808`.
    pub const MIN: BigInt = BigInt(i64::MIN);
    /// The largest value, `9223372036854775807`.
    pub const MAX: BigInt = BigInt(i64::MAX);

    /// Wrap an `i64`.
    #[must_use]
    pub const fn new(value: i64) -> Self {
        Self(value)
    }

    /// The underlying `i64`.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }

    /// `self + rhs`, or `None` on overflow.
    #[must_use]
    pub const fn checked_add(self, rhs: Self) -> Option<Self> {
        match self.0.checked_add(rhs.0) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// `self - rhs`, or `None` on overflow.
    #[must_use]
    pub const fn checked_sub(self, rhs: Self) -> Option<Self> {
        match self.0.checked_sub(rhs.0) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// `self * rhs`, or `None` on overflow.
    #[must_use]
    pub const fn checked_mul(self, rhs: Self) -> Option<Self> {
        match self.0.checked_mul(rhs.0) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

impl From<i64> for BigInt {
    fn from(value: i64) -> Self {
        Self(value)
    }
}

impl From<BigInt> for i64 {
    fn from(value: BigInt) -> Self {
        value.0
    }
}

/// `i64`'s own formatting, so every value written without flags is canonical.
impl fmt::Display for BigInt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, formatter)
    }
}

/// Parses the canonical decimal form only; see [`ParseBigIntError`].
impl FromStr for BigInt {
    type Err = ParseBigIntError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        canonical::parse_canonical(text).map(Self)
    }
}
