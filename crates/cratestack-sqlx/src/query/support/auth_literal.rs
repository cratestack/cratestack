//! `auth().x == <literal>` and `!=`, which both the executed pusher and the
//! preview renderer decide in-process and emit as a SQL constant instead of a
//! comparison. Sharing this one function keeps the two from drifting.
//!
//! The comparison is three-valued (see [`super::comparison`]): a claim of the
//! wrong type is `Unknown`, and what that renders as depends on where the
//! expression sits.
//!
//! - [`Position::Clause`], an allow or deny clause and the `and` / `or` groups
//!   in it: `NULL`. SQL then does with it what it does with any unknown.
//!   `NOT (NULL)` is NULL, which refuses the row, so a `@deny` fires; an
//!   `@allow` on NULL is not true, so it grants nothing. `FALSE`, which this
//!   used to be, left a `@deny` silent: `NOT (FALSE)` is true.
//! - [`Position::Relation`], inside a relation quantifier: `FALSE`. `EXISTS`
//!   never yields NULL, so NULL would buy nothing, and `every` is `NOT EXISTS
//!   (.. AND NOT (expr))`, where a NULL `expr` reads as "no counterexample" and
//!   would grant every parent. A `@deny` over a relation whose inner expression
//!   is undecidable therefore still stays silent.
//!
//! An absent claim (no such key in the context) is not a comparison: it stays
//! `FALSE` in both positions, as it did.

use cratestack_core::Value;

use crate::PolicyLiteral;

use super::comparison::{Truth, claim_vs_literal};

/// Where a policy expression sits, which decides what an undecidable
/// comparison renders as. See the module doc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Position {
    Clause,
    Relation,
}

/// `auth().x == literal`, or `auth().x != literal` when `negate`, as a SQL
/// constant: `TRUE`, `FALSE`, or (undecidable, in a clause) `NULL`.
pub(crate) fn auth_literal_sql(
    claim: Option<&Value>,
    literal: PolicyLiteral,
    negate: bool,
    position: Position,
) -> &'static str {
    let Some(claim) = claim else {
        return "FALSE";
    };
    let comparison = claim_vs_literal(claim, literal);
    let truth = if negate {
        comparison.for_ne()
    } else {
        comparison.for_eq()
    };
    match (truth, position) {
        (Truth::True, _) => "TRUE",
        (Truth::Unknown, Position::Clause) => "NULL",
        (Truth::False | Truth::Unknown, _) => "FALSE",
    }
}
