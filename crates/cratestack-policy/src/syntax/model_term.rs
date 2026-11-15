//! How one model `@@allow`/`@@deny` term is read: which shape it is, and where
//! it divides. The macros lower the result to a `ReadPredicate`; the analysis
//! classifies it. Both call [`read_model_term`], so a term is the same shape
//! to each, and a string literal holding an operator or a keyword
//! (`label in ["== auth().id"]`) is read as the membership test it is.

use super::scan::{find_unquoted, split_comparison};
use super::terms::{
    AUTH_IS_NULL_TERM, AUTH_NOT_NULL_TERM, is_auth_is_system_term, parse_builtin_policy_call,
};

/// One model policy term, by shape. Borrowed slices are of the input term.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelTerm<'a> {
    /// `auth() != null`.
    AuthNotNull,
    /// `auth() == null`.
    AuthIsNull,
    /// `auth().isSystem()`.
    AuthIsSystem,
    /// `hasRole("x")` and kin: `(name, literal)`, or the reason it is invalid.
    Builtin(Result<(&'a str, &'a str), String>),
    /// `auth() == <relation>` or `<relation> == auth()`: the relation, trimmed.
    AuthRelation(&'a str),
    /// `lhs in [list]` / `lhs not in [list]`.
    In {
        /// The field or relation path.
        lhs: &'a str,
        /// The text between the brackets.
        list: &'a str,
        /// `not in`.
        negate: bool,
    },
    /// `lhs == rhs` / `lhs != rhs`, both trimmed.
    Comparison {
        /// Left side.
        lhs: &'a str,
        /// Right side.
        rhs: &'a str,
        /// `!=`.
        negate: bool,
    },
    /// Anything else: a bare boolean field or relation path.
    Bare(&'a str),
}

/// Read one model term. The checks run in the order the macros always did:
/// whole-term `auth()` forms, a builtin call, `auth() ==` / `== auth()`, set
/// membership, then comparison. Membership comes before comparison, and the
/// comparison split ignores literals, so a value cannot pose as an operator.
pub fn read_model_term(term: &str) -> ModelTerm<'_> {
    if term == AUTH_NOT_NULL_TERM {
        return ModelTerm::AuthNotNull;
    }
    if term == AUTH_IS_NULL_TERM {
        return ModelTerm::AuthIsNull;
    }
    if is_auth_is_system_term(term) {
        return ModelTerm::AuthIsSystem;
    }
    if let Some(call) = parse_builtin_policy_call(term) {
        return ModelTerm::Builtin(call);
    }
    if let Some(relation) = term.strip_prefix("auth() ==") {
        return ModelTerm::AuthRelation(relation.trim());
    }
    if let Some(relation) = term.strip_suffix("== auth()") {
        return ModelTerm::AuthRelation(relation.trim());
    }
    if let Some((lhs, list, negate)) = split_in_term(term) {
        return ModelTerm::In { lhs, list, negate };
    }
    if let Some((lhs, rhs, negate)) = split_comparison(term) {
        return ModelTerm::Comparison { lhs, rhs, negate };
    }
    ModelTerm::Bare(term)
}

/// Recognise `lhs in [...]` / `lhs not in [...]`, returning the left-hand
/// side, the bracket contents, and whether it was negated.
///
/// Anchored on the first `[` and the term's final `]` rather than on a
/// substring search for `" in "`, so a value containing the keyword
/// (`purpose in ["shipped in transit"]`) cannot be mis-split.
pub fn split_in_term(term: &str) -> Option<(&str, &str, bool)> {
    let open = find_unquoted(term, "[")?;
    let (head, list) = term.split_at(open);
    let list = list.strip_prefix('[')?.strip_suffix(']')?;

    let head = strip_keyword(head.trim_end(), "in")?;
    match strip_keyword(head, "not") {
        Some(lhs) => Some((lhs, list, true)),
        None => Some((head, list, false)),
    }
}

/// Strip a trailing whole-word `keyword`, returning what precedes it.
///
/// The whitespace check is load-bearing: without it a field named `join`
/// would strip to `jo` and be reported as an unknown model field, which is a
/// confusing way to say "this is not an `in` term".
fn strip_keyword<'a>(head: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = head.strip_suffix(keyword)?;
    if !rest.ends_with(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim_end();
    (!rest.is_empty()).then_some(rest)
}
