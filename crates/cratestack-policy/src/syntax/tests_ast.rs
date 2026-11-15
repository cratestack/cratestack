//! The policy expression grammar, as the macros have always read it: a term
//! runs to the next `&&`/`||` outside every parenthesis, and a parenthesis
//! inside a string literal still counts.

use super::{PolicyAst, parse_policy_ast};

fn term(text: &str) -> PolicyAst {
    PolicyAst::Term(text.to_owned())
}

#[test]
fn parentheses_inside_a_literal_hold_a_term_together() {
    let rule = r#"label == "(x" || published || label == ")""#;
    assert_eq!(parse_policy_ast(rule), Ok(term(rule)));
}

#[test]
fn operators_inside_a_literal_split_the_term() {
    assert_eq!(
        parse_policy_ast(r#"x == "a||b""#),
        Ok(PolicyAst::Or(vec![term(r#"x == "a"#), term(r#"b""#)]))
    );
    assert_eq!(
        parse_policy_ast(r#"label in ["a && b"]"#),
        Ok(PolicyAst::And(vec![
            term(r#"label in ["a"#),
            term(r#"b"]"#)
        ]))
    );
}

#[test]
fn operators_outside_literals_split() {
    assert_eq!(
        parse_policy_ast("a == 1 && b"),
        Ok(PolicyAst::And(vec![term("a == 1"), term("b")]))
    );
}
