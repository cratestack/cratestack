//! How one procedure `@allow`/`@deny` term is read. The procedure dialect has
//! no set membership and no `auth() == relation` form, so its shapes differ
//! from [`super::model_term`]'s; the macros lower the result to a
//! `ProcedurePredicate` and the analysis classifies it, both from
//! [`read_procedure_term`].

use super::scan::split_comparison;
use super::terms::{
    AUTH_IS_NULL_TERM, AUTH_NOT_NULL_TERM, is_auth_is_system_term, parse_builtin_policy_call,
};

/// One procedure policy term, by shape. Borrowed slices are of the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcedureTerm<'a> {
    /// `auth() != null`.
    AuthNotNull,
    /// `auth() == null`.
    AuthIsNull,
    /// A whole-clause `true`.
    True,
    /// A whole-clause `false`.
    False,
    /// `auth().isSystem()`.
    AuthIsSystem,
    /// `hasRole("x")` and kin: `(name, literal)`, or the reason it is invalid.
    Builtin(Result<(&'a str, &'a str), String>),
    /// `lhs == rhs` / `lhs != rhs`, both trimmed.
    Comparison {
        /// Left side.
        lhs: &'a str,
        /// Right side.
        rhs: &'a str,
        /// `!=`.
        negate: bool,
    },
    /// Anything else: a lone boolean input field.
    Bare(&'a str),
}

/// Read one procedure term, in the order the macros always did. The
/// comparison split ignores string literals.
pub fn read_procedure_term(term: &str) -> ProcedureTerm<'_> {
    if term == AUTH_NOT_NULL_TERM {
        return ProcedureTerm::AuthNotNull;
    }
    if term == AUTH_IS_NULL_TERM {
        return ProcedureTerm::AuthIsNull;
    }
    if term == "true" {
        return ProcedureTerm::True;
    }
    if term == "false" {
        return ProcedureTerm::False;
    }
    if is_auth_is_system_term(term) {
        return ProcedureTerm::AuthIsSystem;
    }
    if let Some(call) = parse_builtin_policy_call(term) {
        return ProcedureTerm::Builtin(call);
    }
    if let Some((lhs, rhs, negate)) = split_comparison(term) {
        return ProcedureTerm::Comparison { lhs, rhs, negate };
    }
    ProcedureTerm::Bare(term)
}
