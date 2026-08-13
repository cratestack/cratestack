#![cfg(test)]
//! Two schema-authored names that are distinct as raw identifiers (`myField`
//! vs `my_field`, or `model Foo` vs `model foo`) can still collide once
//! codegen normalizes them via `to_snake_case` — the SQL column name for
//! fields (`cratestack-macros/src/shared/sql.rs`,
//! `.../model/descriptor/columns.rs`), and the table name / Rust accessor
//! constant for model names (`cratestack-macros/src/model/descriptor.rs`,
//! `.../model/accessor.rs`). Left unrejected, this produced valid Rust (two
//! distinct struct fields) but broken duplicate-column DDL for fields, and
//! an opaque `error[E0428]` at the macro call site for model names — see
//! `crates/cratestack-parser/src/validate/snake_case_collisions.rs`.

use crate::parse_schema;

#[test]
fn rejects_case_differing_field_names_that_collide_on_a_model() {
    let error = parse_schema(
        r#"
model Probe {
  id Int @id
  myField String
  my_field String
}
"#,
    )
    .expect_err("`myField` and `my_field` collide on the same SQL column and must be rejected");

    let message = error.to_string();
    assert!(message.contains("myField"), "error: {message}");
    assert!(message.contains("my_field"), "error: {message}");
    assert!(message.contains("Probe"), "error: {message}");
}

#[test]
fn rejects_case_differing_field_names_that_collide_on_a_mixin() {
    let error = parse_schema(
        r#"
mixin Probe {
  myField String
  my_field String
}
"#,
    )
    .expect_err("colliding mixin fields must be rejected");

    assert!(error.to_string().contains("mixin"), "error: {error}");
}

#[test]
fn rejects_case_differing_field_names_that_collide_on_a_type() {
    let error = parse_schema(
        r#"
type Probe {
  myField String
  my_field String
}
"#,
    )
    .expect_err("colliding type-block fields must be rejected");

    assert!(error.to_string().contains("type"), "error: {error}");
}

#[test]
fn rejects_case_differing_field_names_that_collide_on_an_auth_block() {
    let error = parse_schema(
        r#"
auth Probe {
  myField String
  my_field String
}
"#,
    )
    .expect_err("colliding auth block fields must be rejected");

    assert!(error.to_string().contains("auth"), "error: {error}");
}

#[test]
fn rejects_case_differing_field_names_that_collide_on_a_view() {
    let error = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

model Customer {
  id Int @id
  someValue String
}

view Probe from Customer {
  id Int @id @from(Customer.id)
  myField String
  my_field String

  @@server_sql("SELECT id, some_value AS my_field, some_value AS my_field2 FROM customer")
}
"#,
    )
    .expect_err("colliding view fields must be rejected");

    assert!(error.to_string().contains("view"), "error: {error}");
}

#[test]
fn exact_raw_duplicate_field_names_still_hit_the_plain_duplicate_check() {
    // Two fields with the exact same raw name are a pre-existing "duplicate
    // field" error, not this collision check — both normalize to the same
    // name, but they're not *distinct* raw names, so the collision-specific
    // message ("collides with field") must not fire; the older, more
    // direct "duplicate field" message should.
    let error = parse_schema(
        r#"
model Probe {
  id Int @id
  myField String
  myField String
}
"#,
    )
    .expect_err("exact duplicate field names must still be rejected");

    assert!(
        error.to_string().contains("duplicate field"),
        "error: {error}"
    );
}

#[test]
fn a_single_camel_case_field_alone_is_fine() {
    let schema = parse_schema(
        r#"
model Probe {
  id Int @id
  myField String
}
"#,
    )
    .expect("a lone `myField` field has nothing to collide with and must parse fine");

    assert_eq!(schema.models[0].fields[1].name, "myField");
}

#[test]
fn distinct_non_colliding_field_names_are_fine() {
    let schema = parse_schema(
        r#"
model Probe {
  id Int @id
  myField String
  otherField String
}
"#,
    )
    .expect("non-colliding fields must parse fine");

    assert_eq!(schema.models[0].fields.len(), 3);
}

#[test]
fn rejects_case_differing_model_names_that_collide_on_the_table_name() {
    let error = parse_schema(
        r#"
model Foo {
  id Int @id
}

model foo {
  id Int @id
}
"#,
    )
    .expect_err(
        "`Foo` and `foo` normalize to the same generated table name/accessor constant and \
         must be rejected",
    );

    let message = error.to_string();
    assert!(message.contains("Foo"), "error: {message}");
    assert!(message.contains("foo"), "error: {message}");
}

#[test]
fn distinct_model_names_are_fine() {
    let schema = parse_schema(
        r#"
model Foo {
  id Int @id
}

model Bar {
  id Int @id
}
"#,
    )
    .expect("non-colliding model names must parse fine");

    assert_eq!(schema.models.len(), 2);
}

/// `Bus` -> `to_snake_case` `bus` -> `pluralize` `buses` (`s`-ending stems
/// get `es`); `Buse` -> `to_snake_case` `buse` -> `pluralize` `buses` (a
/// bare `s`, since `buse` doesn't end in `s` or a consonant+`y`). Distinct
/// `to_snake_case` forms (so `validate_model_name_collisions` alone lets
/// this through) but the identical pluralized REST route segment
/// `/buses` — the real Axum server panics at startup registering both
/// models' routes, and (pre-fix) `cratestack-mock-wiremock` silently
/// generated two model stubs sharing one route and one state pool with no
/// error at all.
#[test]
fn rejects_model_names_whose_pluralized_route_segments_collide_despite_distinct_snake_case() {
    let error = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

model Bus {
  id Int @id
}

model Buse {
  id Int @id
}
"#,
    )
    .expect_err("`Bus` and `Buse` both route to `/buses` and must be rejected");

    let message = error.to_string();
    assert!(message.contains("Bus"), "error: {message}");
    assert!(message.contains("Buse"), "error: {message}");
    assert!(message.contains("buses"), "error: {message}");
}

#[test]
fn distinct_pluralized_route_segments_are_fine() {
    let schema = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

model Bus {
  id Int @id
}

model Car {
  id Int @id
}
"#,
    )
    .expect("`Bus`/`Car` route to distinct `/buses`/`/cars` segments and must parse fine");

    assert_eq!(schema.models.len(), 2);
}
