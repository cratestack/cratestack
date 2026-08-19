#![cfg(test)]
//! Regression tests for the *derived* half of the builder-name reservation
//! (`validate/builder_collisions.rs`) plus the camelCase spelling of the
//! `build`/`set_build` setter clash (`validate/builder_setter_collisions.rs`).
//!
//! Split from the sibling `tests_builder_collisions` file rather than
//! appended to it, per the repo's ~200-LoC ceiling.
//!
//! Every case here is one an earlier revision of the validator got wrong and
//! shipped as "schema OK" — a declaration name the generator would go on to
//! emit a second time, producing `error[E0428]`/`E0659` at the
//! `include_*_schema!` call site (Rust) or `duplicate_definition` from
//! `dart analyze` (Dart), in both cases with no span pointing at the schema
//! line at fault. The last test is the inverse failure: a *false* rejection
//! the first fix for the others introduced.

use super::parse_schema;

const DATASOURCE: &str = r#"
datasource db {
  provider = "postgresql"
}
"#;

fn expect_rejected(schema: &str, expected_name: &str) {
    let err = parse_schema(&format!("{DATASOURCE}{schema}"))
        .expect_err("collision must be rejected at parse time");
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

/// `Create{M}Input` is generated, not declared, so reserving only declared
/// names left this passing `cratestack check` and then failing as
/// `error[E0659]: CreateTaskInputBuilder is ambiguous` — silently, at
/// whichever downstream crate first named the type.
#[test]
fn derived_create_input_builder_name_rejected() {
    expect_rejected(
        r#"
model Task {
  id Int @id
  name String
}

type CreateTaskInputBuilder {
  other String
}
"#,
        "CreateTaskInputBuilder",
    );
}

/// The Dart generator names this class `{M}FindMany`, dropping the `Input`
/// suffix the Rust struct carries — so reserving only the Rust spelling let
/// `{M}FindManyBuilder` through to `dart analyze` as a duplicate class.
#[test]
fn dart_spelling_find_many_builder_name_rejected() {
    expect_rejected(
        r#"
model Widget {
  id Int @id
  name String
}

type WidgetFindManyBuilder {
  label String
}
"#,
        "WidgetFindManyBuilder",
    );
}

/// A procedure's Dart argument wrapper is `{P}Args` by default, but
/// `procedure_wrapper_name` falls back to `{P}ProcedureArgs` when a
/// declaration already occupies `{P}Args` — here, the `EchoNameArgs` type.
/// Reserving only the default spelling left the fallback exposed.
#[test]
fn procedure_args_fallback_builder_name_rejected() {
    expect_rejected(
        r#"
model Widget {
  id Int @id
  name String
}

type EchoNameArgs {
  x String
}

type EchoNameProcedureArgsBuilder {
  label String
}

procedure echoName(name: String): Widget
"#,
        "EchoNameProcedureArgsBuilder",
    );
}

/// The inverse guard. A procedure generates `{P}Args`, but nothing named
/// `{P}` itself — so a procedure may legitimately be called `WidgetBuilder`
/// even next to `model Widget`. The first fix for the cases above put
/// procedures into the shared entry list and started rejecting this, which
/// would have broken schemas that were valid before the feature existed.
#[test]
fn procedure_named_like_a_model_builder_is_accepted() {
    let schema = format!(
        "{DATASOURCE}{}",
        r#"
model Widget {
  id Int @id
  name String
}

procedure WidgetBuilder(name: String): Widget
"#
    );
    assert!(
        parse_schema(&schema).is_ok(),
        "a procedure named `WidgetBuilder` generates `WidgetBuilderArgs`, not `WidgetBuilder`, \
         so it does not collide with the builder for `model Widget` and must be accepted"
    );
}

/// The Rust setter shim renames a `build` field's setter to `set_build`;
/// the Dart generator renames it to `setBuild`. A literal `"set_build"`
/// comparison therefore missed the camelCase spelling entirely, and
/// `generate-dart` emitted two identical setters while the parser reported
/// "schema OK". The sibling file's tests all use the snake_case spelling,
/// which the pre-fix code caught too — so this is the case that actually
/// pins the normalization.
#[test]
fn camel_case_set_build_field_rejected_alongside_build() {
    expect_rejected(
        r#"
model Gizmo {
  id Int @id
  build String
  setBuild String
}
"#,
        "setBuild",
    );
}
