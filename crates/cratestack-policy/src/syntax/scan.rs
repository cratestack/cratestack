//! Dividing a policy term at its comparison operator.
//!
//! This is the macros' own reading, reproduced exactly: the term is divided at
//! the first `==`, else at the first `!=`, wherever that is. It does not look
//! at string literals, so an operator inside one still divides the term
//! (`x != "a == b"` is read as `==` with `x != "a` on the left). That is how
//! the generated server has always read a policy; `contract ops` reads it the
//! same way so the two cannot disagree. Reading literals better is a
//! deliberate, separate change (see the changelog).

/// Split `term` at its first `==` (else its first `!=`) into
/// `(lhs, rhs, negate)`, both sides trimmed.
///
/// `==` is looked for first, as both dialects always have: a term with both
/// operators divides at the `==`.
pub(crate) fn split_comparison(term: &str) -> Option<(&str, &str, bool)> {
    if let Some((lhs, rhs)) = term.split_once("==") {
        return Some((lhs.trim(), rhs.trim(), false));
    }
    let (lhs, rhs) = term.split_once("!=")?;
    Some((lhs.trim(), rhs.trim(), true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparison_splits_at_the_first_equals_else_the_first_not_equals() {
        assert_eq!(split_comparison("a == b"), Some(("a", "b", false)));
        assert_eq!(split_comparison("a != b"), Some(("a", "b", true)));
        assert_eq!(
            split_comparison("a != b == c"),
            Some(("a != b", "c", false))
        );
        assert_eq!(split_comparison("a"), None);
    }

    /// The quirk the server has always had: a literal is not special.
    #[test]
    fn an_operator_inside_a_literal_still_divides_the_term() {
        assert_eq!(
            split_comparison(r#"x != "a == b""#),
            Some((r#"x != "a"#, r#"b""#, false))
        );
        assert_eq!(
            split_comparison(r#"x == "a == b""#),
            Some(("x", r#""a == b""#, false))
        );
    }
}
