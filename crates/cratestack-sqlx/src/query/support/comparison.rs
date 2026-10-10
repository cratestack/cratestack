//! In-process comparisons behind the create-path policy evaluator and the
//! `auth().x == <literal>` predicates, which are decided at render time
//! instead of in SQL.
//!
//! # Why three-valued
//!
//! Every read, update and delete policy is pushed into SQL, where Postgres
//! decides `col != $1` itself and a NULL or a type mismatch is "unknown",
//! which is not true. The create path has no row to ask, so it compares the
//! prospective input here. It used to spell `!=` as `!matches(..)`, which
//! turns "these two values cannot be compared" into "they differ" and
//! satisfies a negated policy. For `BigInt` that was reachable three ways: a
//! `_ => false` fallthrough in the literal match, derived `PartialEq`
//! between `SqlValue::BigInt(7)` and an auth `SqlValue::Int(7)`, and a
//! canonical-string claim that matches neither (ADR 0019 PR B, risk 2).
//!
//! [`Comparison`] keeps the third outcome apart. `==` and `in` are true only
//! on [`Comparison::Equal`]; `!=` and `not in` only on
//! [`Comparison::Different`]; an [`Comparison::Undecidable`] pair is neither,
//! as in SQL, so an `@allow` built on either does not grant.
//!
//! # A deny fires on Undecidable
//!
//! Reading that as plain `false` fails open for a `@deny`: SQL refuses with
//! `NOT (unknown)`, while a two-valued create path stayed silent. The create
//! evaluator therefore carries a [`Truth`]: an `@allow` grants only on `True`;
//! a `@deny` fires on anything that is not `False`.
//! `and` / `or` combine per Kleene: `False` decides an `and`, `True` decides
//! an `or`, otherwise `Unknown` survives. `in` is the `or` of its `==`, `not
//! in` the `and` of its `!=`.
//!
//! The procedure evaluator states the same rule in
//! `crates/cratestack-policy/src/truth.rs`; its `Truth` is private to that
//! crate, so this module mirrors it. Change one, change the others.
//!
//! An absent operand (no such column in the input, no such claim, a claim that
//! lowers to no `SqlValue`) is not a pair: it stays `False`, as the
//! pushed-down form's `FALSE` constant does.
//!
//! The in-process `auth().x <op> <literal>` of read, update and delete has the
//! same rule: an undecidable claim renders `NULL`, not `FALSE`, so that SQL's
//! `NOT (NULL)` refuses the row; see [`super::auth_literal`].
//!
//! # What is and is not decided
//!
//! A `BigInt` column is compared numerically with an integer literal or an
//! integer claim, so `SqlValue::BigInt(7)` equals `SqlValue::Int(7)`. A
//! `BigInt` against anything else (a string claim, a bool, a NULL, either
//! side) is undecidable. A canonical-string claim is refused here for the
//! same reason Postgres refuses it in the pushed-down form (`bigint = text`
//! has no operator): the predicate carries no column type, so neither path
//! can tell a `BigInt` string from a `String` one. A `BigInt` auth claim
//! must therefore be an integer in a policy comparison; `@default(auth().x)`
//! is the one place that also accepts the canonical string.
//!
//! The pairs that existed before `BigInt` keep their old answers (derived
//! equality for column against claim, so a NULL or a mismatch still counts as
//! different there). Whether those should also be undecidable is a separate
//! change; see the PR B report.

use cratestack_core::Value;

use crate::{PolicyLiteral, SqlValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Comparison {
    Equal,
    Different,
    /// The two sides cannot be compared. Satisfies neither `==` nor `!=`.
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

/// Kleene's three values, mirroring `cratestack_policy`'s private `Truth`. An
/// `@allow` needs `True`; a `@deny` stays silent only on `False`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Truth {
    True,
    False,
    Unknown,
}

impl From<bool> for Truth {
    fn from(value: bool) -> Self {
        if value { Self::True } else { Self::False }
    }
}

impl Truth {
    /// `&&`: decided by a `False` whatever stands beside it.
    pub(crate) fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::False, _) | (_, Self::False) => Self::False,
            (Self::True, Self::True) => Self::True,
            _ => Self::Unknown,
        }
    }

    /// `||`: decided by a `True` whatever stands beside it.
    pub(crate) fn or(self, other: Self) -> Self {
        match (self, other) {
            (Self::True, _) | (_, Self::True) => Self::True,
            (Self::False, Self::False) => Self::False,
            _ => Self::Unknown,
        }
    }

    pub(crate) fn is_true(self) -> bool {
        self == Self::True
    }

    pub(crate) fn is_false(self) -> bool {
        self == Self::False
    }
}

/// A prospective input column against a schema-authored literal. Literal
/// policies exist only on required Boolean, Int, BigInt, String and enum
/// fields, so every other pairing is unreachable from generated code and is
/// undecidable rather than "different".
pub(crate) fn column_vs_literal(value: &SqlValue, literal: PolicyLiteral) -> Comparison {
    match (value, literal) {
        (SqlValue::Bool(left), PolicyLiteral::Bool(right)) => Comparison::of(*left == right),
        (SqlValue::Int(left), PolicyLiteral::Int(right)) => Comparison::of(*left == right),
        // `PolicyLiteral::Int` is an `i64` and a `BigInt` column is an `INT8`.
        (SqlValue::BigInt(left), PolicyLiteral::Int(right)) => Comparison::of(*left == right),
        (SqlValue::String(left), PolicyLiteral::String(right)) => Comparison::of(left == right),
        _ => Comparison::Undecidable,
    }
}

/// An auth claim against a schema-authored literal. Claims are free-form
/// `Value`s from the application's auth provider, and a claim of the wrong
/// runtime type satisfies neither `==` nor `!=`; before `BigInt` it satisfied
/// `!=`, so a `"7"` claim passed `auth().tenantId != 7`.
pub(crate) fn claim_vs_literal(value: &Value, literal: PolicyLiteral) -> Comparison {
    match (value, literal) {
        (Value::Bool(left), PolicyLiteral::Bool(right)) => Comparison::of(*left == right),
        // An `Int` or `BigInt` claim: a JSON integer in the context.
        (Value::Int(left), PolicyLiteral::Int(right)) => Comparison::of(*left == right),
        (Value::String(left), PolicyLiteral::String(right)) => Comparison::of(left == right),
        _ => Comparison::Undecidable,
    }
}

/// A prospective input column against the caller's claim, already lowered to
/// a `SqlValue` by `auth_value_to_sql`.
pub(crate) fn column_vs_claim(candidate: &SqlValue, claim: &SqlValue) -> Comparison {
    match (candidate, claim) {
        // Numeric, not derived `PartialEq`: `BigInt(7)` and `Int(7)` are the
        // same number in different variants.
        (SqlValue::BigInt(left), SqlValue::BigInt(right))
        | (SqlValue::BigInt(left), SqlValue::Int(right))
        | (SqlValue::Int(left), SqlValue::BigInt(right)) => Comparison::of(left == right),
        // A `BigInt` or its NULL on either side against anything else.
        (SqlValue::BigInt(_) | SqlValue::NullBigInt, _)
        | (_, SqlValue::BigInt(_) | SqlValue::NullBigInt) => Comparison::Undecidable,
        // Everything that predates `BigInt`: derived equality.
        (left, right) => Comparison::of(left == right),
    }
}
