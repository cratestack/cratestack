#![cfg(test)]
//! cratestack#398: a field named after a Rust keyword with no valid
//! identifier spelling at all (`self`/`Self`/`super`/`crate`) must be
//! rejected here, at schema-parse time, naming the field and its owner —
//! rather than surfacing as an opaque `rustc` parse error pointing at the
//! `include_server_schema!` macro call site.
//!
//! Every *other* Rust keyword (`match`, `type`, `ref`, `move`, `impl`,
//! `fn`, `let`, `loop`, `box`, ...) has a valid raw-identifier spelling
//! (`r#type`) and must keep parsing successfully — escaping those happens
//! later, at codegen time, in `cratestack_macros::shared::ident`.

use super::parse_schema;

#[test]
fn rejects_self_as_a_model_field_name() {
    let error = parse_schema(
        r#"
model KwProbe {
  id Int @id
  self String
}
"#,
    )
    .expect_err("a field named `self` must be rejected");

    let message = error.to_string();
    assert!(message.contains("self"), "error: {message}");
    assert!(message.contains("KwProbe"), "error: {message}");
    assert!(message.contains("model"), "error: {message}");
}

#[test]
fn rejects_every_unrepresentable_keyword_as_a_model_field_name() {
    for keyword in ["self", "Self", "super", "crate"] {
        let source = format!(
            r#"
model KwProbe {{
  id Int @id
  {keyword} String
}}
"#
        );
        parse_schema(&source).expect_err(&format!("field named `{keyword}` must be rejected"));
    }
}

#[test]
fn rejects_self_as_a_type_block_field_name() {
    let error = parse_schema(
        r#"
type KwProbe {
  self String
}
"#,
    )
    .expect_err("a `type` block field named `self` must be rejected");

    let message = error.to_string();
    assert!(message.contains("self"), "error: {message}");
    assert!(message.contains("KwProbe"), "error: {message}");
    assert!(message.contains("type"), "error: {message}");
}

#[test]
fn rejects_self_as_a_mixin_field_name() {
    let error = parse_schema(
        r#"
mixin KwProbe {
  self String
}
"#,
    )
    .expect_err("a mixin field named `self` must be rejected");

    assert!(error.to_string().contains("mixin"), "error: {error}");
}

#[test]
fn raw_escapable_keywords_parse_successfully_as_model_fields() {
    // cratestack#398's own tested table (minus the four unrepresentable
    // ones covered above): every one of these must still parse — escaping
    // to `r#match`/`r#type`/... happens at codegen time, not here.
    for keyword in [
        "match", "type", "ref", "move", "impl", "fn", "let", "loop", "box",
    ] {
        let source = format!(
            r#"
model KwProbe {{
  id Int @id
  {keyword} String
}}
"#
        );
        let schema = parse_schema(&source)
            .unwrap_or_else(|error| panic!("`{keyword}` should parse fine: {error}"));
        assert_eq!(schema.models[0].fields[1].name, keyword);
    }
}
