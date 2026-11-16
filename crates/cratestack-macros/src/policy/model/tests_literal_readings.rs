//! A policy literal is read exactly as the macros have always read it
//! (cratestack#1388, "no server change"): a term runs to the next `&&`/`||`
//! outside every parenthesis, parentheses inside a literal included, and a
//! comparison divides at the first `==`/`!=` wherever it is. A backslash is an
//! ordinary character and is never unescaped.
//!
//! These rows were taken from the lowering before the shared term reader
//! existed and are pinned here, errors and their exact text included, so a
//! "better" reading of literals cannot slip in under an unrelated change. It
//! is a deliberate, breaking change tracked in its own issue; when it lands,
//! this file changes with it.

use quote::quote;

use super::generate_policies_for_action;

fn lower(rule: &str) -> Result<String, String> {
    let schema = format!(
        r#"
auth SessionUser {{
  id String
  role String
  tier String
}}

model Q {{
  id Int @id
  label String
  x String
  published Boolean

  @@allow("read", {rule})
}}
"#
    );
    let parsed = cratestack_parser::parse_schema(&schema).expect("fixture schema should parse");
    let model = parsed.models.first().expect("fixture declares a model");
    generate_policies_for_action(
        model,
        &parsed.models,
        &parsed.types,
        &parsed.enums,
        parsed.auth.as_ref(),
        "read",
    )
    .map(|policies| policies.iter().map(ToString::to_string).collect())
}

/// `FieldEqLiteral` on `column` holding exactly `value` as a string.
fn field_eq(column: &str, value: &str) -> String {
    let value = quote! { #value }.to_string();
    format!(
        "FieldEqLiteral {{ column : \"{column}\" , value : :: cratestack :: PolicyLiteral :: String ({value}) , }}"
    )
}

#[test]
fn parentheses_inside_literals_hold_a_term_together() {
    // One comparison, not a three-way OR: an anonymous caller sees only the
    // row whose label is that odd string.
    let lowered = lower(r#"label == "(x" || published || label == ")""#).unwrap();
    assert!(
        lowered.contains(&field_eq("label", r#"(x" || published || label == ")"#)),
        "{lowered}"
    );
    assert_eq!(lowered.matches("FieldEqLiteral").count(), 1, "{lowered}");
    assert!(!lowered.contains("FieldIsTrue"), "{lowered}");
}

#[test]
fn a_backslash_is_never_unescaped() {
    for (rule, value) in [
        (r#"label == "q\"x""#, r#"q\"x"#),
        (r#"label == "a\\""#, r"a\\"),
    ] {
        let lowered = lower(rule).unwrap();
        assert!(
            lowered.contains(&field_eq("label", value)),
            "{rule}: {lowered}"
        );
    }
}

#[test]
fn an_operator_inside_a_literal_divides_the_term_and_is_refused() {
    for (rule, error) in [
        (
            r#"label == "q\" && z""#,
            r#"unknown model field `z"` in read policy for `Q`"#,
        ),
        (r#"x == "a||b""#, "expected string literal for field `x`"),
        (
            r#"auth().role == "a&&b""#,
            "expected string literal for field `role`",
        ),
        (
            r#"x != "a == b""#,
            r#"unknown model field `x != "a` in read policy for `Q`"#,
        ),
        (
            r#"label in ["a && b"]"#,
            r#"unknown model field `label in ["a` in read policy for `Q`"#,
        ),
        (
            r#"label == ")" || published"#,
            r#"unexpected trailing policy expression near ')" || published'"#,
        ),
    ] {
        assert_eq!(lower(rule).unwrap_err(), error, "{rule}");
    }
}

/// Membership is read before comparison, so an `==` inside the list is a
/// value; this is what `contract ops` relies on.
#[test]
fn membership_is_read_before_comparison() {
    for rule in [
        r#"label in ["== auth().id", "public"]"#,
        r#"label not in ["!= auth().tier"]"#,
    ] {
        let lowered = lower(rule).unwrap();
        assert!(lowered.contains("InLiterals"), "{rule}: {lowered}");
    }
}
