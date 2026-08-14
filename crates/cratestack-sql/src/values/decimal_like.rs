//! Object-safe decimal payload for [`super::SqlValue::Decimal`]
//! (cratestack#505 Direction 2 — see
//! `docs/design/decimal-backend-additivity.md` §7(b)).
//!
//! `SqlValue` is L1 shared infrastructure: exactly one compiled copy per
//! build, matched exhaustively across `cratestack-sqlx` and
//! `cratestack-rusqlite`. Before this change its `Decimal` variant held
//! the single concrete `cratestack_core::Decimal` alias, which is exactly
//! the union-collision cratestack#505 reports — two independent schemas
//! choosing different backends can't both put their value into the same
//! enum variant naming one fixed type.
//!
//! `Box<dyn DecimalLike>` sidesteps that without making `SqlValue` itself
//! generic (which would have propagated a type parameter through
//! `ModelDescriptor`/`ReadSource`/`WriteSource` and every generated model
//! struct — see the design doc §7's cost discussion of that alternative,
//! "(a)"). Any concrete decimal type can be boxed into this variant; the
//! two backend-specific encode/decode boundaries
//! (`cratestack-sqlx::push_bind_value`, `cratestack-rusqlite`'s TEXT
//! round-trip) downcast back to a concrete type only where they actually
//! need one (sqlx; the rusqlite boundary never needs to downcast at all,
//! since it only ever calls `Display`/`FromStr`).
//!
//! Unconditional — no `#[cfg]` gate, no dependency on `rust_decimal` or
//! `bigdecimal` in this crate. `DecimalLike` blanket-implements for any
//! type satisfying `cratestack_core::DecimalValue`'s bounds, so both
//! concrete backends get it for free the moment `cratestack-core` is in
//! scope, with no per-backend code here.

use std::any::Any;
use std::fmt::{Debug, Display};

use cratestack_core::DecimalValue;

/// Object-safe counterpart to [`DecimalValue`]. `Debug`/`Display` are
/// automatically implemented for `dyn DecimalLike` (supertrait methods are
/// always available on a trait object); `Clone`/`PartialEq` are not
/// object-safe (`Self: Sized`), so this trait provides hand-rolled
/// equivalents (`clone_boxed`, `dyn_eq`) that [`Box<dyn DecimalLike>`]'s
/// own `Clone`/`PartialEq` impls (below) delegate to.
pub trait DecimalLike: Debug + Display + Send + Sync {
    fn clone_boxed(&self) -> Box<dyn DecimalLike>;
    fn dyn_eq(&self, other: &dyn DecimalLike) -> bool;
    fn as_any(&self) -> &dyn Any;
}

impl<T> DecimalLike for T
where
    T: DecimalValue,
{
    fn clone_boxed(&self) -> Box<dyn DecimalLike> {
        Box::new(self.clone())
    }

    fn dyn_eq(&self, other: &dyn DecimalLike) -> bool {
        other
            .as_any()
            .downcast_ref::<T>()
            .is_some_and(|o| self == o)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// `Box<T>` is a "fundamental" type (see the `#[fundamental]` attribute in
// the standard library), which relaxes the orphan rule for exactly this
// shape: `Clone`/`PartialEq` are foreign traits and `Box` is a foreign
// type, but `dyn DecimalLike` is local, so `impl Clone for Box<dyn
// DecimalLike>` is legal. This is the standard pattern for making a boxed
// trait object `Clone`/`PartialEq`.
impl Clone for Box<dyn DecimalLike> {
    fn clone(&self) -> Self {
        self.as_ref().clone_boxed()
    }
}

impl PartialEq for Box<dyn DecimalLike> {
    fn eq(&self, other: &Self) -> bool {
        self.as_ref().dyn_eq(other.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, PartialOrd)]
    struct Fake(i64);

    impl Display for Fake {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl std::str::FromStr for Fake {
        type Err = std::num::ParseIntError;
        fn from_str(s: &str) -> Result<Self, Self::Err> {
            s.parse().map(Fake)
        }
    }

    impl From<i64> for Fake {
        fn from(value: i64) -> Self {
            Fake(value)
        }
    }

    #[test]
    fn boxed_decimal_like_clones_and_compares_by_value() {
        // `assert_eq!`/`assert_ne!` (not `assert!(a == b)`) trip E0507 here —
        // their expansion needs `Box<dyn DecimalLike>: Copy` for the
        // dereference in its match-guard comparison, which a boxed trait
        // object never is. Plain `==`/`!=` inside `assert!` only ever
        // reborrows, so it doesn't hit that path.
        let a: Box<dyn DecimalLike> = Box::new(Fake(42));
        let b = a.clone();
        assert!(a == b);
        let c: Box<dyn DecimalLike> = Box::new(Fake(7));
        assert!(a != c);
    }

    #[test]
    fn boxed_decimal_like_formats_via_display() {
        let a: Box<dyn DecimalLike> = Box::new(Fake(42));
        assert_eq!(a.to_string(), "42");
    }
}
