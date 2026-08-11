//! Decimal scalar.
//!
//! Selected at compile time via mutually-exclusive Cargo features. Generated
//! code references `cratestack::Decimal` regardless of backend, so swapping
//! backends is a workspace-feature flip rather than a code change.
//!
//! The two backends are NOT drop-in equivalents at the trait level:
//! `rust_decimal::Decimal` is `Copy`; `bigdecimal::BigDecimal` is not (it
//! heap-allocates its digit buffer via `num-bigint`). Every call site across
//! the workspace that used to rely on an implicit `Decimal` copy was audited
//! and changed to an explicit `.clone()` as part of cratestack#495 — see
//! `cratestack-sqlx/src/query/support/values.rs`'s `push_bind_value` for the
//! one spot that actually needed it. Both backends do implement `Clone`,
//! `Debug`, `Display`, `FromStr`, `PartialEq`, `PartialOrd`, `Ord`, `Eq`,
//! `Hash`, and `Default`, so no other trait bound in the workspace needed to
//! change.
//!
//! # This mutual exclusivity is a graph-wide invariant, not a per-crate one (cratestack#505)
//!
//! `decimal-rust-decimal` and `decimal-bigdecimal` are mutually exclusive —
//! only ONE `Decimal` type alias can exist, so both selected simultaneously
//! is a hard `compile_error!` below, not a warning. Because Cargo features
//! are additive and unify globally across a dependency graph, this
//! exclusivity is not just a per-crate concern: it is possible for two
//! *independent* dependents, each individually well-formed and each
//! deliberately choosing a different backend, to force this error into a
//! combined build that neither one alone controls or can fix. There is
//! currently no way to avoid this other than the whole graph standardizing on
//! one backend feature; see cratestack#505 for the discussion of making the
//! backends genuinely additive instead (an unresolved, larger design change,
//! deliberately out of scope here — this crate only documents the invariant
//! and softens the *other* half of the same bug report, below).
//!
//! # Selecting NEITHER feature is not an error (cratestack#505)
//!
//! Older versions of this crate also hard-errored when NEITHER decimal
//! feature was selected, which bit a consumer that legitimately used
//! `default-features = false` to narrow its dependency graph and never
//! touched `Decimal` at all — that consumer was nonetheless forced to name a
//! decimal backend it never used, and the break was invisible in a
//! `cargo check --workspace` run (feature unification from other workspace
//! members hid it) until someone built that member alone.
//!
//! Because Cargo's feature system has no "else" — an optional dependency can
//! only be *activated* by a feature that names it, never *auto-activated*
//! by the absence of some other feature — `Decimal` cannot be made to
//! silently resolve to `rust_decimal::Decimal` in this situation without
//! either (a) making `rust_decimal` a mandatory, always-resolved dependency
//! of this crate (which would put `rust_decimal` back in the dependency tree
//! of every `decimal-bigdecimal` consumer too, defeating the whole point of
//! `decimal-bigdecimal` — verified empirically against `cratestack-pg`'s
//! `cargo tree`, cratestack#495's own acceptance bar), or (b) reverting the
//! `default-features = false` convention every internal dependency edge onto
//! this crate uses (which reopens the *original* default-leak bug,
//! cratestack#421). Neither is acceptable, so instead: the `Decimal` type
//! alias, and every unconditional use of it inside this crate (see
//! `validators::validate_range_decimal`), simply does not exist when neither
//! feature is selected — the crate compiles cleanly with a reduced surface
//! instead of hard-erroring. A consumer that never references `Decimal`
//! (directly, or via a schema with no `Decimal`-typed fields) now builds
//! successfully; a consumer that *does* try to use it without picking a
//! backend gets a plain "cannot find type `Decimal`" from rustc instead of
//! this crate's old, clearer `compile_error!` — a minor diagnostic
//! regression accepted in exchange for not hard-failing every backend-
//! agnostic consumer.

#[cfg(all(feature = "decimal-rust-decimal", feature = "decimal-bigdecimal"))]
compile_error!(
    "cratestack: `decimal-rust-decimal` and `decimal-bigdecimal` are mutually exclusive — enable exactly one"
);

#[cfg(all(feature = "decimal-rust-decimal", not(feature = "decimal-bigdecimal")))]
pub type Decimal = rust_decimal::Decimal;

#[cfg(all(feature = "decimal-bigdecimal", not(feature = "decimal-rust-decimal")))]
pub type Decimal = bigdecimal::BigDecimal;

// These tests intentionally reference only `Decimal` (the alias), never a
// backend-specific type, so the exact same suite runs unmodified under
// either `cargo test -p cratestack-core --features decimal-rust-decimal`
// (the default) or `--no-default-features --features decimal-bigdecimal` —
// see `.ci/feature-matrix.sh` for both invocations. `Decimal` doesn't exist
// at all when neither feature is selected (see the module doc above), so
// this module is gated the same way `Decimal` itself is.
#[cfg(all(
    test,
    any(feature = "decimal-rust-decimal", feature = "decimal-bigdecimal")
))]
mod tests {
    use super::*;

    #[test]
    fn decimal_backend_is_available() {
        // Verify that whichever backend is active compiles and the Decimal
        // type works.
        let d = Decimal::from(42);
        assert_eq!(d.to_string(), "42");
    }

    #[test]
    fn decimal_type_arithmetic() {
        // Verify basic decimal operations work
        let d1 = Decimal::from(10);
        let d2 = Decimal::from(20);
        // Just verify the types compile and basic operations work
        let _ = d1 + d2;
    }
}

// cratestack#505 regression coverage: `cargo test -p cratestack-core
// --no-default-features` (neither decimal backend selected) used to hard-
// fail with the "enable exactly one decimal backend" `compile_error!`
// before this feature even reached test collection. This module only
// exists to prove that configuration now builds and runs at all — the
// counterpart to `.ci/feature-matrix.sh`'s "[1/6]" step, which asserts the
// same thing from outside the crate via `cargo check`/`cargo test`. It
// cannot reference `Decimal` (nothing else in this crate can either, in
// this configuration — see the module doc above), so it is gated the
// opposite way `mod tests` above is: only when NEITHER decimal feature is
// active.
#[cfg(all(
    test,
    not(feature = "decimal-rust-decimal"),
    not(feature = "decimal-bigdecimal")
))]
mod no_decimal_backend_tests {
    #[test]
    fn crate_builds_and_runs_with_no_decimal_backend_selected() {
        // Reaching this assertion at all is the regression test: it proves
        // `cratestack-core` compiled (and its test binary linked and ran)
        // with neither `decimal-rust-decimal` nor `decimal-bigdecimal`
        // enabled, which was a hard compile_error! before cratestack#505.
    }
}
