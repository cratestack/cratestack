//! Split out of `decimal.rs` to keep it under this repo's ~200-LoC
//! convention. This file is `decimal::tests`, so each submodule below that
//! needs `decimal.rs`'s own items (`Decimal`, `RustDecimal`, ...) reaches
//! them via `use super::super::*;` (two levels up: submodule -> `tests` ->
//! `decimal`), not `super::*`, which would only reach this file's own
//! (empty) top level. Each `mod` carries its own `#[cfg]` gate — see each
//! one's doc comment for which decimal-feature configuration it covers.

// These tests intentionally reference only `Decimal` (the legacy alias),
// never a backend-specific type, so the exact same suite runs unmodified
// under either `cargo test -p cratestack-core --features decimal-rust-decimal`
// (the default) or `--no-default-features --features decimal-bigdecimal` —
// see `.ci/feature-matrix.sh` for both invocations. `Decimal` doesn't exist
// when neither, or both, features are selected (see `decimal.rs`'s module
// doc), so this module is gated to exactly the "one selected" configurations.
#[cfg(any(
    all(feature = "decimal-rust-decimal", not(feature = "decimal-bigdecimal")),
    all(feature = "decimal-bigdecimal", not(feature = "decimal-rust-decimal"))
))]
mod one_backend_selected_tests {
    use super::super::*;

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
// fail before this feature even reached test collection. This module only
// exists to prove that configuration now builds and runs at all — the
// counterpart to `.ci/feature-matrix.sh`'s "[2/7]" step, which asserts the
// same thing from outside the crate via `cargo check`/`cargo test`.
#[cfg(all(
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

// cratestack#505 Direction 2 regression coverage: both decimal backends
// selected at once used to be a hard `compile_error!` — the issue's own
// headline scenario (two independent dependents, each choosing one
// backend, unified into the same build). This module proves that
// configuration now builds and runs, and that both `RustDecimal` and
// `BigDecimal` are independently usable in it with no ambiguity.
#[cfg(all(feature = "decimal-rust-decimal", feature = "decimal-bigdecimal"))]
mod both_decimal_backends_tests {
    use super::super::*;

    #[test]
    fn both_backends_selected_at_once_compiles_and_runs() {
        // Reaching this assertion at all is the regression test: it proves
        // `cratestack-core` compiled (and its test binary linked and ran)
        // with BOTH `decimal-rust-decimal` and `decimal-bigdecimal`
        // enabled, which was a hard compile_error! before this change.
        let a: RustDecimal = RustDecimal::from(42);
        let b: BigDecimal = BigDecimal::from(42);
        assert_eq!(a.to_string(), b.to_string());
    }

    #[test]
    fn decimal_value_bound_is_satisfied_by_both_backends() {
        fn assert_decimal_value<D: DecimalValue>(value: D) -> String {
            value.to_string()
        }
        assert_eq!(assert_decimal_value(RustDecimal::from(7)), "7");
        assert_eq!(assert_decimal_value(BigDecimal::from(7)), "7");
    }
}
