//! Semantic checks for the model-level `@@internal("action")` attribute
//! (cratestack#743, implementing `docs/design/route-suppression.md`).
//! Parsing/expansion itself is covered by
//! `cratestack_core::schema::internal_attribute`'s own unit tests; here
//! we only assert the schema layer accepts well-formed declarations and
//! rejects the ones an invalid action name would otherwise let through
//! silently to codegen.

#![cfg(test)]

use super::parse_schema;

#[test]
fn accepts_valid_internal_attribute() {
    let schema = parse_schema(
        r#"
model Widget {
  id String @id

  @@internal("create")
}
"#,
    )
    .expect("@@internal(\"create\") should parse");

    assert!(
        schema.models[0]
            .attributes
            .iter()
            .any(|a| a.raw == "@@internal(\"create\")"),
        "attributes: {:?}",
        schema.models[0].attributes,
    );
}

#[test]
fn accepts_multiple_internal_attributes_on_one_model() {
    let schema = parse_schema(
        r#"
model Widget {
  id String @id

  @@internal("create")
  @@internal("update")
}
"#,
    )
    .expect("multiple @@internal(...) lines should parse");

    let internal_count = schema.models[0]
        .attributes
        .iter()
        .filter(|a| a.raw.starts_with("@@internal("))
        .count();
    assert_eq!(internal_count, 2);
}

#[test]
fn rejects_invalid_action_naming_model_and_action() {
    let error = parse_schema(
        r#"
model Widget {
  id String @id

  @@internal("frobnicate")
}
"#,
    )
    .expect_err("an unknown action verb must be a compile error");

    let message = error.to_string();
    assert!(
        message.contains("Widget"),
        "error should name the model `Widget`: {message}"
    );
    assert!(
        message.contains("frobnicate"),
        "error should name the bad action `frobnicate`: {message}"
    );
}

#[test]
fn rejects_malformed_internal_attribute() {
    let error = parse_schema(
        r#"
model Widget {
  id String @id

  @@internal(create)
}
"#,
    )
    .expect_err("an unquoted action must be a compile error");
    assert!(error.to_string().contains("Widget"));
}
