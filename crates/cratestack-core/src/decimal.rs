//! Decimal scalar(s).
//!
//! cratestack#505 Direction 2 (the associated-type/marker shape, §7(b) of
//! `docs/design/decimal-backend-additivity.md`): the two backends are no
//! longer mutually exclusive Cargo features that resolve to one shared
//! `Decimal` alias. Both `decimal-rust-decimal` and `decimal-bigdecimal`
//! may now be selected **simultaneously** in the same build — each backend
//! is exposed under its own unconditional (per-feature) name, [`RustDecimal`]
//! / [`BigDecimal`], so two independent dependents that each choose a
//! different backend no longer force a shared `compile_error!` on each
//! other. `cratestack-macros`' codegen picks *which* of the two a given
//! schema uses via the `decimal = RustDecimal | BigDecimal` argument on
//! the three entry macros (see `cratestack-macros/src/include/decimal_arg.rs`)
//! — a schema-authored choice, not a Cargo feature — so the choice is
//! resolved per invoking crate instead of once, globally, for the whole
//! dependency graph.
//!
//! The legacy `Decimal` alias below is kept for hand-written (non-codegen)
//! call sites that only ever select one backend — it still only exists
//! when *exactly one* feature is active, symmetrically with the "neither"
//! case (see below): naming one ambiguous alias when two backends are both
//! compiled in has no single correct answer, so — exactly like "neither" —
//! it simply isn't exported, rather than silently picking one (§6a of the
//! design doc explains why a silent pick is the wrong failure mode).
//! Generated code never references this alias; it names `RustDecimal` /
//! `BigDecimal` directly.
//!
//! [`DecimalValue`] is the backend-agnostic bound generated code and
//! shared helpers (`validators::validate_range_decimal`) are written
//! against — a blanket impl, unconditional, that both concrete backends
//! (and nothing else already in this crate's dependency graph — see its
//! own doc) satisfy.
//!
//! # Selecting NEITHER feature is not an error (cratestack#505, #521)
//!
//! Older versions of this crate hard-errored when NEITHER decimal feature
//! was selected, which bit a consumer that legitimately used
//! `default-features = false` to narrow its dependency graph and never
//! touched `Decimal` at all. `RustDecimal`/`BigDecimal` (and the legacy
//! `Decimal` alias) simply don't exist when their backing feature isn't
//! selected — the crate compiles cleanly with a reduced surface instead of
//! hard-failing every backend-agnostic consumer.
//!
//! # Selecting BOTH features is also not an error (cratestack#505)
//!
//! This is the actual fix this module implements. Previously: `Decimal`
//! could only ever be one alias, so both features active at once was a
//! hard `compile_error!` — and because Cargo features are additive and
//! unify globally across a dependency graph, two *independent* dependents,
//! each individually well-formed and each deliberately choosing a
//! different backend, could force that error into a combined build that
//! neither one alone controlled or could fix. `RustDecimal` and
//! `BigDecimal` are distinct names, each gated on its own feature
//! independently (not on "exactly one") — both may exist in the same
//! compiled `cratestack-core` at once with no ambiguity, because nothing
//! has to pick between them at this layer. The choice moves to the schema
//! author, via `cratestack-macros`' `decimal = ...` macro argument — see
//! this module's top doc.

/// Backend-agnostic bound for a decimal scalar. Blanket-implemented for
/// any type satisfying these bounds — deliberately structural rather than
/// naming `rust_decimal::Decimal` / `bigdecimal::BigDecimal` explicitly, so
/// this trait (and everything written against it, e.g.
/// `validators::validate_range_decimal`) compiles unconditionally, with no
/// `#[cfg]` gate of its own and no dependency on either optional backend
/// crate.
///
/// `From<i64>` is required so `@range` integer bounds
/// (`amount Decimal @range(min: 0)`) can be promoted to the field's own
/// decimal type for comparison — both `rust_decimal::Decimal` and
/// `bigdecimal::BigDecimal` implement it.
pub trait DecimalValue:
    Clone
    + std::fmt::Debug
    + std::fmt::Display
    + std::str::FromStr
    + PartialEq
    + PartialOrd
    + From<i64>
    + Send
    + Sync
    + 'static
{
}

impl<T> DecimalValue for T where
    T: Clone
        + std::fmt::Debug
        + std::fmt::Display
        + std::str::FromStr
        + PartialEq
        + PartialOrd
        + From<i64>
        + Send
        + Sync
        + 'static
{
}

/// rust_decimal: 96-bit fixed-precision, stack-allocated, `Copy`. Fast, no
/// heap allocation, capped at 28-29 significant decimal digits. Only
/// exists under the `decimal-rust-decimal` Cargo feature (independent of
/// whether `decimal-bigdecimal` is also active — see this module's doc).
#[cfg(feature = "decimal-rust-decimal")]
pub use rust_decimal::Decimal as RustDecimal;

/// bigdecimal: arbitrary-precision, heap-allocated (backed by
/// `num-bigint`), NOT `Copy`. Chosen when a schema's monetary/precision
/// requirements can exceed `RustDecimal`'s 28-29 significant digits, at
/// the cost of an allocation per value. Only exists under the
/// `decimal-bigdecimal` Cargo feature (independent of whether
/// `decimal-rust-decimal` is also active — see this module's doc).
#[cfg(feature = "decimal-bigdecimal")]
pub use bigdecimal::BigDecimal;

/// Legacy single-backend alias, kept for hand-written call sites outside
/// generated code. Only exists when *exactly one* backend feature is
/// active — see this module's doc for why "both" doesn't pick a default
/// instead of simply not exporting this name.
#[cfg(all(feature = "decimal-rust-decimal", not(feature = "decimal-bigdecimal")))]
pub type Decimal = rust_decimal::Decimal;

#[cfg(all(feature = "decimal-bigdecimal", not(feature = "decimal-rust-decimal")))]
pub type Decimal = bigdecimal::BigDecimal;

#[cfg(test)]
mod tests;
