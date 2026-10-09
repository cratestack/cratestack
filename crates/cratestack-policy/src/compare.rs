//! Comparing two values in a procedure policy: a claim, an argument or a
//! literal against another.
//!
//! # Why an integer and a string are not compared
//!
//! A `BigInt` procedure argument reaches the evaluator as `Value::Int`
//! (`cratestack-macros` `shared::value_tokens`). The claim it is compared
//! with comes from the application's auth provider, and a `BigInt` claim above
//! 2^53 has to be a string to survive a JavaScript issuer. Derived equality
//! calls `Value::String("7")` and `Value::Int(7)` different, so
//! `owner != auth().accountId` was TRUE for a caller whose claim was `"7"`, and
//! the policy meant to refuse that caller let them through.
//!
//! The string is not read as a number to repair that. A predicate carries no
//! type, so the evaluator cannot tell a `BigInt` string from a `String` one,
//! and the SQL path cannot either: a string claim renders as `$N::text`, and
//! Postgres has no `bigint = text` operator, so the database refuses it.
//! Reading it as a number here would make the same claim pass in a procedure
//! and fail in a model. A `BigInt` claim has to be an integer in a policy
//! comparison; only `@default(auth().x)` also takes the canonical string.
//!
//! # The rule
//!
//! A `Value::Int` against a `Value::String` (or against a string literal, or a
//! `Value::String` against an integer literal), in either order, whether the
//! sides are a claim, an argument or a literal, is
//! [`Comparison::Undecidable`]: neither equal nor different. `==` does not pass
//! and `!=` does not pass. As a [`Truth`] it is `Unknown`, so a `@deny` on the
//! same comparison fires, as the same comparison does in SQL.
//!
//! Every other pair is what it always was, with no third outcome: equal when
//! the derived `==` says so, different otherwise. That includes integer
//! against integer, string against string (`"007"` still differs from `"7"`),
//! two arguments of the same type, and a `Bool`, `Float` or `Null` against
//! either (a known gap: they still pass `!=`).
//!
//! This must give the outcome `cratestack-sqlx` gives the same claim on the
//! create path, in `crates/cratestack-sqlx/src/query/support/comparison.rs`.
//! Change one, change the other. That file also makes the other mismatched
//! pairs undecidable; this one leaves them.
//!
//! The procedure dialect has only `==` and `!=`: there is no `in`, `not in`
//! or ordering predicate here (`ProcedurePredicate`), so those operators have
//! nothing to decide in this module.

use cratestack_core::Value;

use crate::procedure_types::ProcedurePolicyLiteral;
use crate::truth::Truth;

/// The outcome of comparing two values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Comparison {
    Equal,
    Different,
    /// The two cannot be compared (an integer against a string). Satisfies
    /// neither `==` nor `!=`.
    Undecidable,
}

impl Comparison {
    fn of(equal: bool) -> Self {
        if equal { Self::Equal } else { Self::Different }
    }

    /// The truth of `left == right`.
    pub(crate) fn for_eq(self) -> Truth {
        match self {
            Self::Equal => Truth::True,
            Self::Different => Truth::False,
            Self::Undecidable => Truth::Unknown,
        }
    }

    /// The truth of `left != right`.
    pub(crate) fn for_ne(self) -> Truth {
        match self {
            Self::Equal => Truth::False,
            Self::Different => Truth::True,
            Self::Undecidable => Truth::Unknown,
        }
    }
}

/// Two runtime values: a claim, or an argument, against another.
pub(crate) fn compare_values(left: &Value, right: &Value) -> Comparison {
    match (left, right) {
        (Value::Int(_), Value::String(_)) | (Value::String(_), Value::Int(_)) => {
            Comparison::Undecidable
        }
        _ => Comparison::of(left == right),
    }
}

/// A runtime value (a claim or an argument) against a schema-authored literal.
pub(crate) fn compare_literal(value: &Value, literal: ProcedurePolicyLiteral) -> Comparison {
    match (value, literal) {
        (Value::Bool(left), ProcedurePolicyLiteral::Bool(right)) => Comparison::of(*left == right),
        (Value::Int(left), ProcedurePolicyLiteral::Int(right)) => Comparison::of(*left == right),
        (Value::String(left), ProcedurePolicyLiteral::String(right)) => {
            Comparison::of(left == right)
        }
        (Value::Int(_), ProcedurePolicyLiteral::String(_))
        | (Value::String(_), ProcedurePolicyLiteral::Int(_)) => Comparison::Undecidable,
        _ => Comparison::Different,
    }
}
