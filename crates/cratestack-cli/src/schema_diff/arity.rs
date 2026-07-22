//! Shared arity-change classification, used by both field diffing and
//! procedure arg/return-type diffing.

use cratestack_core::TypeArity;

use super::Severity;

/// Widening (`required` → `optional`) is additive; anything else —
/// tightening to `required`, or introducing/removing a list — is
/// breaking, since it invalidates existing callers' assumptions about
/// the value's shape.
pub(super) fn classify_arity_change(from: TypeArity, to: TypeArity) -> Severity {
    if matches!((from, to), (TypeArity::Required, TypeArity::Optional)) {
        Severity::Additive
    } else {
        Severity::Breaking
    }
}

pub(super) fn arity_label(arity: TypeArity) -> &'static str {
    match arity {
        TypeArity::Required => "required",
        TypeArity::Optional => "optional",
        TypeArity::List => "list",
    }
}
