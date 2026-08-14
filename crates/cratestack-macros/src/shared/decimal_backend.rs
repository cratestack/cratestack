//! Ambient (thread-scoped) context for "which concrete decimal type is
//! this schema using", read by the small number of codegen sites that
//! need to name a concrete `Decimal` type (cratestack#505 Direction 2 —
//! see `docs/design/decimal-backend-additivity.md` §7(b) and
//! `crate::include::decimal_arg`, which parses the macro-level
//! `decimal = RustDecimal | BigDecimal` argument this module's setter is
//! fed from).
//!
//! Per cratestack#505 §5: a proc-macro can only observe *its own*
//! compiled-in Cargo feature set via `cfg!`, never the invoking crate's —
//! so the concrete decimal type can't be chosen by a `cfg!` check inside
//! this crate. It has to come from data the macro invocation itself
//! carries (the `decimal = ...` argument), which is exactly what gets
//! stored here for the duration of one schema's composition.
//!
//! A scoped thread-local rather than an explicit parameter threaded
//! through `rust_type_tokens`/`sql_value_tokens`/etc.'s ~30 call sites:
//! those functions are called from many unrelated composers (primary-key
//! types, enum types, ...) that never touch `Decimal` at all, and adding
//! a parameter to every one of them (most of which would just pass it
//! through unused) is exactly the signature-noise cost
//! `docs/design/decimal-backend-additivity.md` §7 warns generic-parameter
//! Direction 2(a) would inflict on *generated* code — this keeps that
//! noise out of `cratestack-macros`' own internals too. [`with_decimal_backend`]
//! is the only way to set it, and it always restores the previous value
//! (nests correctly if macro expansion is ever re-entrant on one thread).

use std::cell::Cell;

use quote::quote;

/// Which concrete decimal type a schema's `decimal = ...` macro argument
/// named. Mirrors `cratestack_core::{RustDecimal, BigDecimal}` — see
/// `crate::include::decimal_arg::DecimalBackend`, the macro-argument
/// parse type this is filled in from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DecimalBackend {
    RustDecimal,
    BigDecimal,
}

thread_local! {
    static CURRENT: Cell<Option<DecimalBackend>> = const { Cell::new(None) };
}

/// Runs `f` with `backend` as the active decimal backend, restoring
/// whatever was active before on the way out. Every one of the three
/// entry-macro composers (`include::server`/`embedded`/`client`) wraps its
/// entire schema-composition body in exactly one call to this.
pub(crate) fn with_decimal_backend<R>(backend: Option<DecimalBackend>, f: impl FnOnce() -> R) -> R {
    let previous = CURRENT.with(|cell| cell.replace(backend));
    let result = f();
    CURRENT.with(|cell| cell.set(previous));
    result
}

/// Token path for the field type / turbofish argument codegen should
/// name for a `Decimal`-typed field: `::cratestack::RustDecimal` or
/// `::cratestack::BigDecimal`, whichever the enclosing
/// [`with_decimal_backend`] scope selected. Panics if called with no
/// scope active — every one of the six call sites that call this is only
/// ever reached while composing a schema that
/// `crate::include::decimal_arg::schema_uses_decimal` already confirmed
/// declared a `decimal = ...` argument (composition fails at
/// argument-validation time otherwise, before any of these sites run).
pub(crate) fn current_decimal_type_tokens() -> proc_macro2::TokenStream {
    match CURRENT.with(Cell::get) {
        Some(DecimalBackend::RustDecimal) => quote! { ::cratestack::RustDecimal },
        Some(DecimalBackend::BigDecimal) => quote! { ::cratestack::BigDecimal },
        None => unreachable!(
            "current_decimal_type_tokens() called with no decimal backend scope active — \
             every call site is only reached for a schema that `schema_uses_decimal` \
             already confirmed requires `decimal = RustDecimal | BigDecimal`, which the \
             entry-macro composer validates before entering the `with_decimal_backend` scope"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nests_and_restores_the_previous_value() {
        with_decimal_backend(Some(DecimalBackend::RustDecimal), || {
            assert_eq!(
                current_decimal_type_tokens().to_string(),
                quote! { ::cratestack::RustDecimal }.to_string()
            );
            with_decimal_backend(Some(DecimalBackend::BigDecimal), || {
                assert_eq!(
                    current_decimal_type_tokens().to_string(),
                    quote! { ::cratestack::BigDecimal }.to_string()
                );
            });
            // Restored after the nested scope exits.
            assert_eq!(
                current_decimal_type_tokens().to_string(),
                quote! { ::cratestack::RustDecimal }.to_string()
            );
        });
    }

    #[test]
    #[should_panic(expected = "no decimal backend scope active")]
    fn panics_with_no_scope_active() {
        with_decimal_backend(None, current_decimal_type_tokens);
    }
}
