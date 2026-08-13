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
//!
//! The tests below the `--- generalized coverage ---` marker extend this
//! to every other ident site `cratestack-macros` feeds unguarded into
//! `ident()`/`to_snake_case()`: enum names/variants, top-level
//! model/mixin/type/view/procedure names, and procedure argument names —
//! previously only field names were covered (see the review that found
//! this gap: `enum Status { self, active }` used to compile all the way
//! to an opaque `error: expected identifier, found keyword \`self\`` at
//! the macro call site).

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

// --- generalized coverage ---

#[test]
fn rejects_self_as_an_enum_name() {
    let error = parse_schema(
        r#"
enum self {
  active
  inactive
}
"#,
    )
    .expect_err("an enum named `self` must be rejected");

    let message = error.to_string();
    assert!(message.contains("self"), "error: {message}");
    assert!(message.contains("enum"), "error: {message}");
}

#[test]
fn rejects_self_as_an_enum_variant_name() {
    let error = parse_schema(
        r#"
enum Status {
  self
  active
}
"#,
    )
    .expect_err("an enum variant named `self` must be rejected");

    let message = error.to_string();
    assert!(message.contains("self"), "error: {message}");
    assert!(message.contains("Status"), "error: {message}");
}

#[test]
fn rejects_self_as_a_top_level_model_name() {
    let error = parse_schema(
        r#"
model self {
  id Int @id
}
"#,
    )
    .expect_err("a model named `self` must be rejected");

    let message = error.to_string();
    assert!(message.contains("self"), "error: {message}");
    assert!(message.contains("model"), "error: {message}");
}

#[test]
fn rejects_self_as_a_top_level_mixin_name() {
    let error = parse_schema(
        r#"
mixin self {
  createdAt DateTime
}
"#,
    )
    .expect_err("a mixin named `self` must be rejected");

    assert!(error.to_string().contains("mixin"), "error: {error}");
}

#[test]
fn rejects_self_as_a_top_level_type_name() {
    let error = parse_schema(
        r#"
type self {
  label String
}
"#,
    )
    .expect_err("a `type` block named `self` must be rejected");

    assert!(error.to_string().contains("type"), "error: {error}");
}

#[test]
fn rejects_self_as_a_view_name() {
    let error = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

model Customer {
  id Int @id
}

view self from Customer {
  id Int @id @from(Customer.id)

  @@server_sql("SELECT id FROM customer")
}
"#,
    )
    .expect_err("a view named `self` must be rejected");

    assert!(error.to_string().contains("view"), "error: {error}");
}

#[test]
fn rejects_self_as_a_procedure_name() {
    let error = parse_schema(
        r#"
procedure self(): Int
"#,
    )
    .expect_err("a procedure named `self` must be rejected");

    let message = error.to_string();
    assert!(message.contains("self"), "error: {message}");
    assert!(message.contains("procedure"), "error: {message}");
}

#[test]
fn rejects_every_unrepresentable_keyword_as_a_procedure_name() {
    for keyword in ["self", "Self", "super", "crate"] {
        let source = format!("procedure {keyword}(): Int\n");
        parse_schema(&source).expect_err(&format!("procedure named `{keyword}` must be rejected"));
    }
}

#[test]
fn rejects_self_as_a_procedure_argument_name() {
    let error = parse_schema(
        r#"
procedure getFeed(self: Int): Int
"#,
    )
    .expect_err("a procedure argument named `self` must be rejected");

    let message = error.to_string();
    assert!(message.contains("self"), "error: {message}");
    assert!(message.contains("procedure argument"), "error: {message}");
    assert!(message.contains("getFeed"), "error: {message}");
}

#[test]
fn rejects_every_unrepresentable_keyword_as_a_procedure_argument_name() {
    for keyword in ["self", "Self", "super", "crate"] {
        let source = format!("procedure getFeed({keyword}: Int): Int\n");
        parse_schema(&source).expect_err(&format!(
            "procedure argument named `{keyword}` must be rejected"
        ));
    }
}

#[test]
fn substring_matches_of_reserved_keywords_are_not_rejected() {
    // `selfie` contains `self` as a substring but is not equal to it;
    // `crater` contains `crate`. Neither is a Rust keyword, so both must
    // keep parsing fine — the check is exact-match, not substring.
    let schema = parse_schema(
        r#"
model Probe {
  id Int @id
  selfie String
  crater String
}
"#,
    )
    .expect("`selfie`/`crater` fields are not reserved keywords and must parse");

    assert_eq!(schema.models[0].fields[1].name, "selfie");
    assert_eq!(schema.models[0].fields[2].name, "crater");
}

#[test]
fn ordinary_top_level_names_are_not_rejected() {
    // A plain schema using only ordinary identifiers for every ident site
    // this module now covers must keep parsing successfully.
    let schema = parse_schema(
        r#"
enum Status {
  active
  inactive
}

model Widget {
  id Int @id
  status Status
}

mixin Timestamps {
  createdAt DateTime
}

type Summary {
  count Int
}

procedure getWidget(id: Int): Widget
"#,
    )
    .expect("an ordinary schema should parse and validate fine");

    assert_eq!(schema.enums[0].name, "Status");
    assert_eq!(schema.models[0].name, "Widget");
    assert_eq!(schema.mixins[0].name, "Timestamps");
    assert_eq!(schema.types[0].name, "Summary");
    assert_eq!(schema.procedures[0].name, "getWidget");
}
