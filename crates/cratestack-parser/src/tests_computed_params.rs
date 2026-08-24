#![cfg(test)]

//! `@computed(params: <Type>?)` — the parameterized form of `@computed`.
//! Bare `@computed` coverage lives in `tests_computed`/`tests_types`;
//! this file is scoped to the params argument: acceptance, the
//! required-`?` rule, garbage argument forms, and the schema-wide
//! params-type resolution rules in `validate::computed`.

use super::parse_schema;

#[test]
fn accepts_params_form_on_a_type_field() {
    let schema = parse_schema(
        r#"
type ProxyParams {
  width Int?
}

type Thumbnail {
  storageKey String
  url String @computed(params: ProxyParams?)
}
"#,
    )
    .expect("@computed(params: ProxyParams?) on a type field should parse");

    assert_eq!(
        schema.types[1].fields[1].attributes[0].raw,
        "@computed(params: ProxyParams?)"
    );
}

#[test]
fn accepts_params_form_on_a_model_field() {
    let schema = parse_schema(
        r#"
type ProxyParams {
  width Int?
}

model Image {
  id Int @id
  storageKey String
  proxyUrl String @computed(params: ProxyParams?)
}
"#,
    )
    .expect("@computed(params: ProxyParams?) on a model field should parse");

    assert_eq!(
        schema.models[0].fields[2].attributes[0].raw,
        "@computed(params: ProxyParams?)"
    );
}

#[test]
fn rejects_required_params_missing_question_mark() {
    let error = parse_schema(
        r#"
type ProxyParams {
  width Int?
}

type Thumbnail {
  url String @computed(params: ProxyParams)
}
"#,
    )
    .expect_err("@computed(params: ProxyParams) without `?` should fail validation");

    let message = error.to_string();
    assert!(
        message.contains("required computed params are not supported yet"),
        "message: {message}"
    );
    assert!(message.contains("add a trailing `?`"), "message: {message}");
}

#[test]
fn rejects_undeclared_params_type() {
    let error = parse_schema(
        r#"
type Thumbnail {
  url String @computed(params: Missing?)
}
"#,
    )
    .expect_err("an undeclared params type should fail validation");

    assert!(
        error
            .to_string()
            .contains("is not declared anywhere in this schema")
    );
}

#[test]
fn rejects_builtin_scalar_as_params_type() {
    let error = parse_schema(
        r#"
type Thumbnail {
  url String @computed(params: Int?)
}
"#,
    )
    .expect_err("a builtin scalar params type should fail validation");

    assert!(
        error
            .to_string()
            .contains("is a builtin scalar, not a declared `type` block")
    );
}

#[test]
fn rejects_enum_as_params_type() {
    let error = parse_schema(
        r#"
enum Color {
  Red
  Blue
}

type Thumbnail {
  url String @computed(params: Color?)
}
"#,
    )
    .expect_err("an enum params type should fail validation");

    assert!(
        error
            .to_string()
            .contains("is an enum, not a declared `type` block")
    );
}

#[test]
fn rejects_model_as_params_type() {
    let error = parse_schema(
        r#"
model ProxyParams {
  id Int @id
}

type Thumbnail {
  url String @computed(params: ProxyParams?)
}
"#,
    )
    .expect_err("a model params type should fail validation");

    assert!(
        error
            .to_string()
            .contains("is a model, not a declared `type` block")
    );
}

#[test]
fn rejects_computed_bearing_params_type() {
    let error = parse_schema(
        r#"
type ProxyParams {
  width Int?
  label String @computed
}

type Thumbnail {
  url String @computed(params: ProxyParams?)
}
"#,
    )
    .expect_err("a computed-bearing params type should fail validation");

    assert!(
        error
            .to_string()
            .contains("itself contains `@computed` fields")
    );
}

#[test]
fn rejects_garbage_argument_form_unrelated_keyword() {
    let error = parse_schema(
        r#"
type Thumbnail {
  url String @computed(lazy)
}
"#,
    )
    .expect_err("@computed(lazy) should fail validation");

    assert!(error.to_string().contains("use bare `@computed`"));
}

#[test]
fn rejects_garbage_argument_form_missing_colon() {
    let error = parse_schema(
        r#"
type ProxyParams {
  width Int?
}

type Thumbnail {
  url String @computed(params ProxyParams?)
}
"#,
    )
    .expect_err("@computed(params ProxyParams?) missing a colon should fail validation");

    assert!(error.to_string().contains("use bare `@computed`"));
}

#[test]
fn rejects_params_form_combined_with_another_attribute() {
    let error = parse_schema(
        r#"
type ProxyParams {
  width Int?
}

model Image {
  id Int @id
  proxyUrl String @computed(params: ProxyParams?) @readonly
}
"#,
    )
    .expect_err("@computed(params: ProxyParams?) combined with @readonly should fail validation");

    assert!(
        error
            .to_string()
            .contains("combines `@computed` with `@readonly`")
    );
}
