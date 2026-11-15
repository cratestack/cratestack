//! A string literal is a value, never an operator or a keyword
//! (cratestack#1388, second-round review). The server renders
//! `label IN ($1, $2)` for `label in ["== auth().id", "public"]`, so an
//! anonymous caller gets rows; reading the `==` inside the literal as a
//! comparison with `auth()` would report the op as needing a principal.

use super::{PolicyDialect, analyse_op_policy};

fn model(rule: &str) -> super::OpPolicyAnalysis {
    analyse_op_policy(PolicyDialect::Model, &[rule], &[])
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

#[test]
fn an_escaped_quote_does_not_end_the_literal() {
    let verdict = model(r#"label in ["q\" == auth().id"]"#);
    assert!(!verdict.requires_principal);
    assert!(verdict.unanalysed.is_empty());
}

#[test]
fn and_or_inside_a_literal_do_not_split_the_rule() {
    for rule in [
        r#"label in ["a && auth() != null"]"#,
        r#"label in ["a || b"]"#,
        r#"label == "a && b""#,
    ] {
        let verdict = model(rule);
        assert!(!verdict.requires_principal, "{rule}");
        assert!(verdict.unanalysed.is_empty(), "{rule}");
    }
}

#[test]
fn a_literal_holding_an_operator_on_the_right_of_a_comparison_is_one_comparison() {
    // Divides at the real `!=`, not the `==` inside the literal.
    for dialect in [PolicyDialect::Model, PolicyDialect::Procedure] {
        let verdict = analyse_op_policy(dialect, &[r#"label != "a == b""#], &[]);
        assert!(!verdict.requires_principal, "{dialect:?}");
        assert!(verdict.unanalysed.is_empty(), "{dialect:?}");
        let verdict = analyse_op_policy(dialect, &[r#"auth().tier != "a == b""#], &[]);
        assert!(verdict.requires_principal, "{dialect:?}");
    }
}

#[test]
fn a_real_auth_comparison_still_needs_a_principal() {
    assert!(model(r#"label == auth().id"#).requires_principal);
    assert!(model(r#"auth().tier != "free""#).requires_principal);
}
