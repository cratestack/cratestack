#![cfg(test)]
//! Regression tests for [`crate::validate::builder_setter_collisions::validate_no_add_setter_collision`]
//! (issue #661) — the append-setter half of the builder-name reservation.
//!
//! Split out of the sibling `tests_builder_collisions_derived` file rather
//! than appended to it, to stay inside the repo's ~200-LoC ceiling (that
//! file was already at 151 lines).
//!
//! `cratestack-macros/src/builder/fields.rs::build_spec` derives every
//! list-arity field's append setter mechanically as `add_{field.name}`
//! (Rust) / `add{Field}` (Dart, capitalized) — no singularization. A field
//! set that declares both a list field and a real field landing on that
//! same generated name produces two identically-named setters: the
//! generator would go on to emit `error[E0592]: duplicate definitions with
//! name `add_tags`` (Rust) or a `duplicate_definition` from `dart analyze`
//! (Dart), in both cases with no span pointing at the schema line at
//! fault. These tests pin the parse-time rejection instead.

use super::parse_schema;

// Deliberately no `datasource` block: `cratestack-parser`'s
// `validate_field_list_arity_support` rejects a scalar list-valued model
// field (`tags String[]`) on any schema that declares a `datasource` —
// there is no SQL bind representation for a list column — so a
// datasource-bearing fixture would fail for that unrelated reason before
// ever reaching the append-setter collision check these tests pin. A
// schema reachable only through `include_client_schema!` never binds SQL
// values, so the restriction doesn't apply; see
// `crates/cratestack-client/tests/fixtures/builder_pattern.cstack`'s own
// comment for the precedent.

fn expect_rejected(schema: &str, expected_name: &str) {
    let err = parse_schema(schema).expect_err("collision must be rejected at parse time");
    let message = err.to_string();
    assert!(
        message.contains("collides with"),
        "expected a collision diagnostic, got: {message}"
    );
    assert!(
        message.contains(expected_name),
        "diagnostic must name `{expected_name}`, got: {message}"
    );
}

/// The Rust generator spells the append setter for `tags` (a list field)
/// `add_tags` literally — a sibling field declared with that exact name
/// collides with it.
#[test]
fn snake_case_add_field_rejected_alongside_list_field() {
    expect_rejected(
        r#"
model Post {
  id Int @id
  tags String[]
  add_tags String
}
"#,
        "add_tags",
    );
}

/// Same clash, camelCase spelling — the Dart generator spells the same
/// setter `addTags`, so a field literally named `addTags` collides in Dart
/// even though it doesn't literally match the Rust spelling `add_tags`.
/// `to_snake_case` must normalize both onto the same comparison, the same
/// way the `build`/`set_build` check already does for `setBuild`.
#[test]
fn camel_case_add_field_rejected_alongside_list_field() {
    expect_rejected(
        r#"
model Post {
  id Int @id
  tags String[]
  addTags String
}
"#,
        "addTags",
    );
}

/// The inverse guard: `add_foo` beside a *scalar* `foo` must keep parsing.
/// A scalar field generates no append setter at all — the reserved name
/// `add_foo` only exists when `foo` is list-arity. Getting this over-broad
/// (rejecting `add_x` beside any `x`, list or not) is a real regression:
/// an earlier revision of the sibling `builder_collisions.rs` validator
/// made exactly this mistake for a different collision class (falsely
/// rejecting `procedure WidgetBuilder` next to `model Widget`) and had to
/// be reverted.
#[test]
fn add_field_beside_scalar_field_of_same_base_name_is_accepted() {
    let schema = r#"
model Post {
  id Int @id
  foo String
  add_foo String
}
"#;
    assert!(
        parse_schema(schema).is_ok(),
        "`foo` is scalar, not list-arity, so it generates no `.add_foo(..)` setter at all — a \
         field literally named `add_foo` does not collide with anything and must be accepted"
    );
}
