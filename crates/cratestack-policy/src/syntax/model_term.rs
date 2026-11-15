//! How one model `@@allow`/`@@deny` term is read: which shape it is, and where
//! it divides. The macros lower the result to a `ReadPredicate`; the analysis
//! classifies it. Both call [`read_model_term`], so a term is the same shape
//! to each. The order of the checks is the macros' original one, quirks
//! included; in particular `label in ["== auth().id"]` is a membership test
//! because membership is read before comparison.

use super::scan::split_comparison;
use super::terms::{
    AUTH_IS_NULL_TERM, AUTH_NOT_NULL_TERM, is_auth_is_system_term, parse_builtin_policy_call,
};

/// One model policy term, by shape. Borrowed slices are of the input term.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
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
/// membership, then comparison. Membership comes before comparison, so a list
/// whose literals hold `==` is still a membership test. The comparison split
/// is a plain first-`==`/first-`!=` split and does not look at literals.
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
    // Kept as on main, but not reachable: a term ending in `== auth()` holds
    // a `(` and ends in `)`, which is the shape `parse_builtin_policy_call`
    // takes above (`author == auth()` reads as a call named `author == auth`
    // with no argument), so it never gets here. Left in place so the reader
    // stays identical to main's.
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
pub(crate) fn split_in_term(term: &str) -> Option<(&str, &str, bool)> {
    let open = term.find('[')?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_a_plain_in_term() {
        assert_eq!(
            split_in_term("purpose in [product_image, kyc_selfie]"),
            Some(("purpose", "product_image, kyc_selfie", false))
        );
    }

    #[test]
    fn splits_a_negated_in_term() {
        assert_eq!(
            split_in_term("purpose not in [kyc_selfie]"),
            Some(("purpose", "kyc_selfie", true))
        );
    }

    /// The whitespace guard in `strip_keyword`: a field whose name merely
    /// *ends* in `in` is not an `in` term.
    #[test]
    fn a_field_named_join_is_not_an_in_term() {
        assert_eq!(split_in_term("join [a]"), None);
    }

    #[test]
    fn a_term_without_brackets_is_not_an_in_term() {
        assert_eq!(split_in_term("purpose == product_image"), None);
    }

    /// Membership is read before comparison: this is the whole difference
    /// between a set test and a comparison with a garbled left side.
    #[test]
    fn membership_is_read_before_comparison() {
        assert_eq!(
            read_model_term(r#"label in ["== auth().id"]"#),
            ModelTerm::In {
                lhs: "label",
                list: r#""== auth().id""#,
                negate: false
            }
        );
        assert_eq!(
            read_model_term(r#"label not in ["!= x"]"#),
            ModelTerm::In {
                lhs: "label",
                list: r#""!= x""#,
                negate: true
            }
        );
    }

    #[test]
    fn a_comparison_divides_at_the_first_operator_literal_or_not() {
        assert_eq!(
            read_model_term(r#"x != "a == b""#),
            ModelTerm::Comparison {
                lhs: r#"x != "a"#,
                rhs: r#"b""#,
                negate: false
            }
        );
    }
}
