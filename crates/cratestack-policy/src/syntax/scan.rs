//! Finding an operator in a policy term, outside string literals.
//!
//! A literal is `"…"` or `'…'`; a backslash inside one escapes the next
//! character, so `\"` does not close it. An operator that sits inside a
//! literal is part of a value, never an operator: `label in ["== auth().id"]`
//! is a set-membership test against two strings, and `x != "a == b"` is one
//! `!=`. Every splitter of a term goes through here, so the macros that lower
//! a term and the analysis that classifies it cannot disagree on where it
//! divides.

/// The byte offset of the first `needle` in `text` that is outside every
/// string literal.
pub fn find_unquoted(text: &str, needle: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (offset, ch) in text.char_indices() {
        match quote {
            Some(open) => {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == open {
                    quote = None;
                }
            }
            None => {
                if ch == '"' || ch == '\'' {
                    quote = Some(ch);
                } else if text[offset..].starts_with(needle) {
                    return Some(offset);
                }
            }
        }
    }
    None
}

/// Split `term` at its first unquoted `==` (else its first unquoted `!=`)
/// into `(lhs, rhs, negate)`, both sides trimmed.
///
/// `==` is looked for first, as both dialects always have: a term with both
/// operators divides at the `==`.
pub fn split_comparison(term: &str) -> Option<(&str, &str, bool)> {
    [("==", false), ("!=", true)]
        .into_iter()
        .find_map(|(operator, negate)| {
            let at = find_unquoted(term, operator)?;
            Some((
                term[..at].trim(),
                term[at + operator.len()..].trim(),
                negate,
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_operator_inside_a_literal_is_not_found() {
        assert_eq!(find_unquoted(r#"a == "x""#, "=="), Some(2));
        assert_eq!(find_unquoted(r#"a in ["== x"]"#, "=="), None);
        assert_eq!(find_unquoted(r#"a in ['!= x']"#, "!="), None);
        assert_eq!(find_unquoted(r#"a == "b" == c"#, "=="), Some(2));
        assert_eq!(find_unquoted(r#""a" == b"#, "=="), Some(4));
    }

    #[test]
    fn an_escaped_quote_does_not_close_the_literal() {
        assert_eq!(find_unquoted(r#"a in ["q\" == x"]"#, "=="), None);
        assert_eq!(find_unquoted(r#"a != "q\\" == x"#, "=="), Some(11));
    }

    #[test]
    fn the_other_quote_kind_inside_a_literal_is_text() {
        assert_eq!(find_unquoted(r#"a in ["it's", "== x"]"#, "=="), None);
    }

    #[test]
    fn comparison_splits_at_equals_first_and_outside_literals() {
        assert_eq!(split_comparison("a == b"), Some(("a", "b", false)));
        assert_eq!(split_comparison("a != b"), Some(("a", "b", true)));
        assert_eq!(
            split_comparison(r#"a != "x == y""#),
            Some(("a", r#""x == y""#, true))
        );
        assert_eq!(split_comparison(r#"a in ["== x"]"#), None);
        assert_eq!(
            split_comparison("a != b == c"),
            Some(("a != b", "c", false))
        );
    }
}
