//! The policy expression grammar around string literals.

use super::{PolicyAst, parse_policy_ast};

#[test]
fn operators_and_parentheses_inside_a_literal_are_part_of_the_term() {
    for term in [
        r#"label in ["a && b", "c || d"]"#,
        r#"label == "x) || (y""#,
        r#"label in ["q\" && b"]"#,
        r#"label == 'a && b'"#,
    ] {
        assert_eq!(
            parse_policy_ast(term),
            Ok(PolicyAst::Term(term.to_owned())),
            "{term}"
        );
    }
}

#[test]
fn operators_outside_literals_still_split() {
    assert_eq!(
        parse_policy_ast(r#"a == "x && y" && b"#),
        Ok(PolicyAst::And(vec![
            PolicyAst::Term(r#"a == "x && y""#.to_owned()),
            PolicyAst::Term("b".to_owned()),
        ]))
    );
}
