#![cfg(test)]
//! Regression coverage for #229: list-arity scalar/enum model fields
//! (`String[]`, `MyEnum[]`, …) used to pass `check`, get valid Postgres DDL
//! from `cratestack-migrate`, and only then panic `include_server_schema!` /
//! `include_embedded_schema!` at macro expansion — three layers away from
//! the field that actually causes the problem, with a message naming
//! neither the model, the field, nor the type.
//!
//! These tests exercise `parse_schema`, which runs the exact
//! `validate::validate_schema` path `cratestack-cli check` calls into
//! (`parse_schema_file` -> `parse_schema_named` -> `validate_schema`), so a
//! green run here is a faithful proxy for `cratestack-cli check` itself.

use super::parse_schema;

/// The issue's own repro fixture, `tags String[]` on a `datasource`-bearing
/// model: this used to print "schema OK" and only fail three layers later,
/// at `include_server_schema!` expansion, with an unspanned
/// `panic!("unsupported SQLx value type for this slice")` that names
/// neither the model, the field, nor the type. It must now fail at `check`
/// time instead, with a spanned diagnostic that names all three.
#[test]
fn rejects_list_arity_scalar_field_on_datasource_backed_model() {
    let error = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

auth Operator {
  id Int
}

model Tagged {
  id Int @id
  tags String[]

  @@allow("read", auth() != null)
  @@allow("create", auth() != null)
}
"#,
    )
    .expect_err("list-arity scalar field on a database-backed model should be rejected by check");

    let message = error.to_string();
    assert!(
        message.contains("Tagged"),
        "error should name the model: {message}"
    );
    assert!(
        message.contains("tags"),
        "error should name the field: {message}"
    );
    assert!(
        message.contains("String[]"),
        "error should name the offending type: {message}",
    );
}

/// Sibling of the scalar case: an enum-typed list field hits a dedicated
/// panic (`"unsupported SQLx enum list type for this slice"`) further down
/// the same catch-all-shaped code, but must be rejected at the same point
/// for the same reason.
#[test]
fn rejects_list_arity_enum_field_on_datasource_backed_model() {
    let error = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

enum MyEnum {
  first
  second
}

model Tagged {
  id Int @id
  moods MyEnum[]
}
"#,
    )
    .expect_err("list-arity enum field on a database-backed model should be rejected by check");

    let message = error.to_string();
    assert!(
        message.contains("Tagged"),
        "error should name the model: {message}"
    );
    assert!(
        message.contains("moods"),
        "error should name the field: {message}"
    );
    assert!(
        message.contains("MyEnum[]"),
        "error should name the offending type: {message}",
    );
}

/// Same as above but on the `sqlite` provider — `include_embedded_schema!`
/// has exactly the same gap (`cratestack-rusqlite`'s `ToSql` has no array
/// case either), so the rejection must not be Postgres-specific.
#[test]
fn rejects_list_arity_scalar_field_on_sqlite_datasource_backed_model() {
    let error = parse_schema(
        r#"
datasource db {
  provider = "sqlite"
  url = env("DATABASE_URL")
}

model Tagged {
  id Int @id
  tags String[]
}
"#,
    )
    .expect_err("list-arity scalar field on a sqlite-backed model should be rejected by check");

    assert!(error.to_string().contains("Tagged"), "error: {error}",);
}

/// Negative control, proving the fix is scoped correctly rather than a
/// blanket ban: a schema with **no** `datasource` is only ever consumed via
/// `include_client_schema!`, which never binds SQL values, so a list-arity
/// scalar/enum model field is genuinely fine there. This mirrors
/// `crates/cratestack-client-dart/tests/fixtures/enums.cstack`'s
/// `model User { roles Role[] }`, which is exercised as a passing test
/// today (`generates_real_dart_enums_for_schema_enum_fields_and_procedures`)
/// — a version of this fix that rejected list arity unconditionally would
/// have broken that test.
#[test]
fn accepts_list_arity_field_on_client_only_schema_without_datasource() {
    parse_schema(
        r#"
enum Role {
  admin
  member
}

model User {
  id Int @id
  roles Role[]
}
"#,
    )
    .expect("list-arity field on a schema with no datasource (client-only) should still parse");
}

/// Negative control for the relation carve-out: a to-many `@relation` field
/// is also `TypeArity::List` at the IR level (see
/// `cratestack-macros/src/relation/types.rs`'s `is_to_many`), and has real,
/// working codegen support unrelated to `SqlValue`. It must remain
/// accepted even on a `datasource`-bearing schema.
#[test]
fn accepts_to_many_relation_list_field_on_datasource_backed_model() {
    parse_schema(
        r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model User {
  id Int @id
  sessions Session[] @relation(fields:[id],references:[userId])
}

model Session {
  id Int @id
  userId Int
}
"#,
    )
    .expect("a to-many @relation list field should remain accepted");
}
