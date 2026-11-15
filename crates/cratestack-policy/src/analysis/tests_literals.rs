//! The analysis reads a policy exactly as the generated server does, quirks
//! included (cratestack#1388): membership before comparison, a comparison at
//! the first `==`/`!=` wherever it is, a term running to the next `&&`/`||`
//! outside every parenthesis, string literals' own parentheses counted.
//!
//! The server renders `label IN ($1, $2)` for `label in ["== auth().id",
//! "public"]`, so an anonymous caller gets rows; reading the `==` inside the
//! literal as a comparison with `auth()` would report the op as needing a
//! principal. Reading literals better than that is a separate, breaking
//! change, tracked outside this crate.

use super::{PolicyDialect, analyse_op_policy};

fn model(rule: &str) -> super::OpPolicyAnalysis {
    analyse_op_policy(PolicyDialect::Model, &[rule], &[])
}

fn procedure(rule: &str) -> super::OpPolicyAnalysis {
    analyse_op_policy(PolicyDialect::Procedure, &[rule], &[])
}

#[test]
fn membership_is_read_before_comparison() {
    for rule in [
        r#"label in ["== auth().id", "public"]"#,
        r#"label not in ["!= auth().tier"]"#,
        r#"label in ["a", "== auth().id"]"#,
        r#"label in ['== auth().id']"#,
    ] {
        let verdict = model(rule);
        assert!(!verdict.requires_principal, "{rule}");
        assert!(verdict.unanalysed.is_empty(), "{rule}");
    }
}

/// The membership-before-comparison order is the only thing that makes the
/// terms above analysable: read as a comparison they divide at the `==` inside
/// the list and leave a left side no field can have. A mutant that swaps the
/// order fails here, not just on the verdict.
#[test]
fn only_the_order_decides_membership_versus_comparison() {
    use crate::syntax::{ModelTerm, read_model_term};
    for term in [r#"label in ["== auth().id"]"#, r#"label not in ["!= x"]"#] {
        assert!(
            matches!(read_model_term(term), ModelTerm::In { .. }),
            "{term}"
        );
        // What the comparison split alone would make of it.
        let (lhs, _) = term
            .split_once("==")
            .or_else(|| term.split_once("!="))
            .unwrap();
        assert!(
            lhs.contains('['),
            "{term}: the comparison reading is garbled"
        );
    }
    // The comparison reading of that garble is refused, so only the order
    // makes the list analysable.
    let garbled = model(r#"label in ["== auth().id"] == auth().id"#);
    assert!(!garbled.unanalysed.is_empty());
}

/// A backslash is an ordinary character in a policy literal (the server never
/// unescapes), and a quote still closes it.
#[test]
fn a_backslash_does_not_escape_a_quote_in_the_in_list_splitter() {
    let verdict = model(r#"label in ["q\", "== auth().id"]"#);
    assert!(verdict.unanalysed.is_empty());
    assert!(!verdict.requires_principal);
}

#[test]
fn a_list_the_macros_refuse_is_unanalysed() {
    for rule in [r#"label in []"#, r#"label in ["a",]"#, r#"label in ["a]"#] {
        assert!(!model(rule).unanalysed.is_empty(), "{rule}");
    }
}

/// The server reads `label == "(x" || published || label == ")"` as one
/// comparison: the parentheses inside the literals hold the term open, so the
/// `||` between them never splits. The value is the odd string between the
/// outer quotes, and the rule is satisfiable by an anonymous caller.
#[test]
fn parentheses_inside_literals_hold_a_term_together() {
    let verdict = model(r#"label == "(x" || published || label == ")""#);
    assert!(!verdict.requires_principal);
    assert!(verdict.unanalysed.is_empty());
}

/// Operators inside a literal split a term where the server splits it, so
/// what the server then refuses at compile time is reported, not guessed.
#[test]
fn an_operator_inside_a_literal_divides_the_term_as_the_server_does() {
    for dialect_rule in [
        r#"x == "a||b""#,
        r#"label == "a && b""#,
        r#"x != "a == b""#,
        r#"auth().tier != "a == b""#,
        r#"label == "q\" && z""#,
    ] {
        assert!(!model(dialect_rule).unanalysed.is_empty(), "{dialect_rule}");
        assert!(
            !procedure(dialect_rule).unanalysed.is_empty(),
            "{dialect_rule}"
        );
    }
}

#[test]
fn a_real_auth_comparison_still_needs_a_principal() {
    assert!(model(r#"label == auth().id"#).requires_principal);
    assert!(model(r#"auth().tier != "free""#).requires_principal);
}
