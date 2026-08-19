#![cfg(test)]
//! Tests for `validate_builder_name_collisions` (`validate/builder_collisions.rs`):
//! a schema declaring both `X` and `XBuilder` must be rejected at parse
//! time rather than surfacing as an opaque `E0428`/`E0659` at the
//! `include_*_schema!` call site — see the review finding that motivated
//! this file (a `type Foo` + `type FooBuilder` schema compiled cleanly on
//! `main` before every struct-shaped generated type gained a builder).

use super::parse_schema;

#[test]
fn type_vs_same_named_type_builder_rejected() {
    let err = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

type Foo {
  value String
}

type FooBuilder {
  other String
}

model User {
  id Int @id
  name String
}
"#,
    )
    .expect_err("type `FooBuilder` collides with the builder generated for type `Foo`");

    let message = err.to_string();
    assert!(message.contains("collides with"), "error: {message}");
    assert!(message.contains("FooBuilder"), "error: {message}");
    assert!(message.contains("Foo"), "error: {message}");
}

#[test]
fn model_vs_type_builder_cross_module_rejected() {
    // `type Order` emits `types::OrderBuilder`; `model OrderBuilder` emits
    // `models::OrderBuilder`. Both are glob re-exported to the parent
    // module (`pub use types::*; pub use models::*;`), so this is the
    // E0659-ambiguous-glob-import case, not same-module E0428.
    let err = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

type Order {
  total Int
}

model OrderBuilder {
  id Int @id
  label String
}
"#,
    )
    .expect_err("model `OrderBuilder` collides with the builder generated for type `Order`");

    let message = err.to_string();
    assert!(message.contains("collides with"), "error: {message}");
    assert!(message.contains("OrderBuilder"), "error: {message}");
}

#[test]
fn view_vs_model_builder_rejected() {
    // Views are emitted into the `models` module alongside model structs
    // (both server and embedded — see the doc comment on
    // `Entry::generates_builder`), so a view can collide with a model's
    // generated builder too.
    let err = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

model Widget {
  id Int @id
  name String
}

view WidgetBuilder from Widget {
  id Int @from(Widget.id)
  name String @from(Widget.name)
}
"#,
    )
    .expect_err("view `WidgetBuilder` collides with the builder generated for model `Widget`");

    let message = err.to_string();
    assert!(message.contains("collides with"), "error: {message}");
    assert!(message.contains("WidgetBuilder"), "error: {message}");
}

#[test]
fn enum_named_like_a_builder_is_rejected_as_target() {
    // Enums never generate their own builder, but they still occupy the
    // shared `types` namespace, so they can be the *target* half of a
    // collision even though they can never be the *source* half.
    let err = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

type Status {
  label String
}

enum StatusBuilder {
  ACTIVE
}

model User {
  id Int @id
  name String
}
"#,
    )
    .expect_err("enum `StatusBuilder` collides with the builder generated for type `Status`");

    let message = err.to_string();
    assert!(message.contains("collides with"), "error: {message}");
    assert!(message.contains("StatusBuilder"), "error: {message}");
}

#[test]
fn model_build_and_set_build_fields_rejected() {
    // `setter_ident` renames a `build` field's setter to `set_build`
    // (`cratestack-macros/src/builder/emit.rs`) so it doesn't collide with
    // the terminal `build()` method — but that rename itself collides with
    // a real `set_build` field's own setter.
    let err = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

model Thing {
  id Int @id
  build String
  set_build String
}
"#,
    )
    .expect_err("`build` + `set_build` fields on the same model must be rejected");

    let message = err.to_string();
    assert!(message.contains("build"), "error: {message}");
    assert!(message.contains("set_build"), "error: {message}");
}

#[test]
fn type_build_and_set_build_fields_rejected() {
    let err = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

type Thing {
  build String
  set_build String
}

model User {
  id Int @id
  name String
}
"#,
    )
    .expect_err("`build` + `set_build` fields on the same type must be rejected");

    let message = err.to_string();
    assert!(message.contains("build"), "error: {message}");
    assert!(message.contains("set_build"), "error: {message}");
}

#[test]
fn procedure_build_and_set_build_args_rejected() {
    let err = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

type Ack {
  ok Boolean
}

procedure doThing(build: String, set_build: String): Ack
"#,
    )
    .expect_err("`build` + `set_build` procedure args must be rejected");

    let message = err.to_string();
    assert!(message.contains("build"), "error: {message}");
    assert!(message.contains("set_build"), "error: {message}");
}

#[test]
fn model_build_field_alone_is_allowed() {
    // A lone `build` field is handled by the `set_build` rename with no
    // collision — must not be rejected.
    parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

model Thing {
  id Int @id
  build String
}
"#,
    )
    .expect("a lone `build` field is fine — the rename has nothing to collide with");
}

#[test]
fn model_find_many_input_rust_spelling_collision_rejected() {
    // The Rust-side struct name (`cratestack-macros/src/model/
    // find_many_input.rs`) — must stay caught alongside the Dart spelling
    // below.
    let err = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

model Widget {
  id Int @id
  name String
}

type WidgetFindManyInputBuilder {
  label String
}
"#,
    )
    .expect_err(
        "type `WidgetFindManyInputBuilder` collides with the Rust `{Model}FindManyInput` builder",
    );

    let message = err.to_string();
    assert!(message.contains("collides with"), "error: {message}");
    assert!(
        message.contains("WidgetFindManyInputBuilder"),
        "error: {message}"
    );
}

#[test]
fn model_find_many_dart_spelling_collision_rejected() {
    // cratestack-client-dart's `find_many_views.rs` names this generated
    // class `{Model}FindMany` (no `Input` suffix) — a schema declaring
    // `{Model}FindManyBuilder` used to pass `cratestack check` and then
    // fail Dart codegen with a `duplicate_definition` `dart analyze`
    // error, since only the Rust spelling was reserved.
    let err = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

model Widget {
  id Int @id
  name String
}

type WidgetFindManyBuilder {
  label String
}
"#,
    )
    .expect_err("type `WidgetFindManyBuilder` collides with the Dart `{Model}FindMany` builder");

    let message = err.to_string();
    assert!(message.contains("collides with"), "error: {message}");
    assert!(
        message.contains("WidgetFindManyBuilder"),
        "error: {message}"
    );
}

#[test]
fn procedure_args_builder_collision_rejected() {
    // cratestack-client-dart emits a top-level `{PascalCase(name)}Args`
    // class per procedure (unlike the Rust side, which scopes `Args`
    // inside `pub mod <procedure>` and never collides) — a schema
    // declaring `{Proc}ArgsBuilder` used to pass `cratestack check` and
    // then fail Dart codegen with a `duplicate_definition` error, since
    // procedures were never scanned for builder-name reservations at all.
    let err = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

type Ack {
  ok Boolean
}

type EchoNameArgsBuilder {
  label String
}

procedure echoName(name: String): Ack
"#,
    )
    .expect_err("type `EchoNameArgsBuilder` collides with the Dart `{Proc}Args` builder");

    let message = err.to_string();
    assert!(message.contains("collides with"), "error: {message}");
    assert!(message.contains("EchoNameArgsBuilder"), "error: {message}");
}

#[test]
fn procedure_args_builder_camel_case_name_collision_rejected() {
    // Exercises the shared `to_pascal_case` step itself: a snake_case-ish
    // procedure name still maps to the same PascalCase symbol Dart emits.
    let err = parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

type Ack {
  ok Boolean
}

type FetchUserProfileArgsBuilder {
  label String
}

procedure fetch_user_profile(id: String): Ack
"#,
    )
    .expect_err("type `FetchUserProfileArgsBuilder` collides with the Dart args builder");

    let message = err.to_string();
    assert!(message.contains("collides with"), "error: {message}");
    assert!(
        message.contains("FetchUserProfileArgsBuilder"),
        "error: {message}"
    );
}

#[test]
fn unrelated_builder_suffixed_name_is_allowed() {
    // A name that merely *ends* in `Builder` is fine as long as no other
    // declaration's generated builder actually collides with it.
    parse_schema(
        r#"
datasource db {
  provider = "postgresql"
}

model QueryBuilder {
  id Int @id
  name String
}
"#,
    )
    .expect("no colliding `Query` declaration exists — must not be rejected");
}
