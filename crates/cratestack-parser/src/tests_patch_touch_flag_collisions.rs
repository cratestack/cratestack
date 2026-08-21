#![cfg(test)]
//! Regression tests for
//! [`crate::validate::patch_touch_flag_collisions::validate_no_touch_flag_collision`]
//! (cratestack#663 follow-up) — the `{field}IsSet` touch-flag half of the
//! generated-Dart-identifier reservation, the same defect class as the
//! sibling `build`/`set_build` and `add_{field}` checks in
//! `tests_builder_add_setter_collisions.rs`.
//!
//! `cratestack-client-dart/src/patch_touch.rs` mechanically derives a
//! sibling `{field}IsSet` bool for every `TypeArity::Optional` field that
//! lands in a model's generated `Update{Model}Input` Dart class. A schema
//! that also declares a real field named `fooIsSet` alongside a nullable
//! `foo` gets two Dart members fighting over that identifier — `dart
//! analyze` surfaces it as eight separate errors with no span pointing back
//! at the schema line at fault. These tests pin the parse-time rejection
//! instead, and the inverse: a non-nullable (or relation, or primary-key)
//! field named `fooIsSet` generates no touch flag at all and must keep
//! parsing.

use super::parse_schema;

fn expect_rejected(schema: &str, expected_name: &str) {
    let err =
        parse_schema(schema).expect_err("touch-flag collision must be rejected at parse time");
    let message = err.to_string();
    assert!(
        message.contains("generated identifier"),
        "expected a touch-flag collision diagnostic, got: {message}"
    );
    assert!(
        message.contains(expected_name),
        "diagnostic must name `{expected_name}`, got: {message}"
    );
}

/// The collision this check exists for: a nullable `weight` alongside a
/// real field literally named `weightIsSet`, the exact identifier
/// `cratestack-client-dart` mechanically derives for `weight`'s own touch
/// flag on `UpdateWidgetInput`.
#[test]
fn nullable_field_rejected_alongside_its_own_touch_flag_name() {
    expect_rejected(
        r#"
model Widget {
  id Int @id
  weight Int?
  weightIsSet Boolean
}
"#,
        "weightIsSet",
    );
}

/// The inverse guard: `fooIsSet` beside a *non-nullable* `foo` must keep
/// parsing. A `Required`-arity field generates no touch flag at all (see
/// `crate::patch_touch`'s module doc — only `TypeArity::Optional` patch
/// fields get one), so there is no generated identifier for `fooIsSet` to
/// collide with.
#[test]
fn touch_flag_named_field_beside_non_nullable_field_of_same_base_name_is_accepted() {
    let schema = r#"
model Widget {
  id Int @id
  weight Int
  weightIsSet Boolean
}
"#;
    assert!(
        parse_schema(schema).is_ok(),
        "`weight` is non-nullable, so it generates no `weightIsSet` touch flag at all — a field \
         literally named `weightIsSet` does not collide with anything and must be accepted"
    );
}

/// `fooIsSet` with no `foo` field at all in the same model must also keep
/// parsing — the collision check must never fire on a name that merely
/// happens to end in `IsSet` with nothing colliding behind it.
#[test]
fn touch_flag_named_field_with_no_matching_base_field_is_accepted() {
    let schema = r#"
model Widget {
  id Int @id
  weightIsSet Boolean
}
"#;
    assert!(
        parse_schema(schema).is_ok(),
        "`weightIsSet` has no sibling nullable `weight` field to collide with and must be accepted"
    );
}

/// The primary-key exclusion: `Update{Model}Input` never includes the `@id`
/// field itself (it's the immutable identity of the row being patched), so
/// a nullable `@id` field generates no touch flag to collide with — even
/// though it is otherwise `TypeArity::Optional`.
#[test]
fn nullable_primary_key_field_beside_its_touch_flag_name_is_accepted() {
    let schema = r#"
model Widget {
  id Int? @id
  idIsSet Boolean
}
"#;
    assert!(
        parse_schema(schema).is_ok(),
        "`id` is the primary key, excluded from `Update{{Model}}Input` entirely, so it generates \
         no `idIsSet` touch flag and must be accepted"
    );
}

/// The relation-field exclusion: a nullable relation field is dropped from
/// `Update{Model}Input` by `scalar_model_fields` before the touch flag is
/// ever derived (relations aren't patched through the scalar update input),
/// so it generates no touch flag to collide with.
#[test]
fn nullable_relation_field_beside_its_touch_flag_name_is_accepted() {
    let schema = r#"
model Author {
  id Int @id
  name String
}

model Post {
  id Int @id
  title String
  authorId Int?
  author Author? @relation(fields:[authorId],references:[id])
  authorIsSet Boolean
}
"#;
    assert!(
        parse_schema(schema).is_ok(),
        "`author` is a relation field, dropped from `UpdatePostInput` entirely, so it generates \
         no `authorIsSet` touch flag and must be accepted"
    );
}
