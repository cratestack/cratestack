#![cfg(test)]

use super::parse_schema;

#[test]
fn accepts_model_emit_attribute() {
    let schema = parse_schema(
        r#"
model Session {
  id Cuid @id

  @@emit(created, deleted)
}
"#,
    )
    .expect("model emit attribute should parse");

    assert_eq!(
        schema.models[0].attributes[0].raw,
        "@@emit(created, deleted)"
    );
}

#[test]
fn rejects_invalid_model_emit_attribute_operation() {
    let error = parse_schema(
        r#"
model Session {
  id Cuid @id

  @@emit(created, archived)
}
"#,
    )
    .expect_err("unknown event operation should fail validation");

    assert!(
        error
            .to_string()
            .contains("unsupported event operation `archived`")
    );
}

#[test]
fn accepts_bare_model_paged_attribute() {
    let schema = parse_schema(
        r#"
model Session {
  id Cuid @id

  @@paged
}
"#,
    )
    .expect("bare @@paged should parse");

    assert_eq!(schema.models[0].attributes[0].raw, "@@paged");
}

#[test]
fn rejects_invalid_model_paged_attribute_forms() {
    let error = parse_schema(
        r#"
model Session {
  id Cuid @id

  @@paged(mode: "offset")
}
"#,
    )
    .expect_err("configured @@paged should fail validation");

    assert!(error.to_string().contains("use bare `@@paged`"));
}

#[test]
fn accepts_soft_delete_and_retain_attributes() {
    let schema = parse_schema(
        r#"
model Customer {
  id Int @id
  email String

  @@soft_delete
  @@retain(days: 2555)
}
"#,
    )
    .expect("model with soft-delete + retain should parse");

    let attrs = &schema.models[0].attributes;
    assert!(attrs.iter().any(|a| a.raw == "@@soft_delete"));
    assert!(attrs.iter().any(|a| a.raw == "@@retain(days: 2555)"));
}

#[test]
fn rejects_retain_without_days_argument() {
    let error = parse_schema(
        r#"
model Customer {
  id Int @id

  @@retain(weeks: 12)
}
"#,
    )
    .expect_err("@@retain(weeks: 12) should fail");

    assert!(
        error.to_string().contains("`@@retain` requires `days: N`"),
        "error: {error}",
    );
}

#[test]
fn rejects_soft_delete_with_args() {
    let error = parse_schema(
        r#"
model Customer {
  id Int @id

  @@soft_delete(column: "deleted")
}
"#,
    )
    .expect_err("@@soft_delete(...) should fail");

    assert!(
        error.to_string().contains("does not take arguments"),
        "error: {error}",
    );
}

#[test]
fn accepts_audit_attribute_on_model() {
    let schema = parse_schema(
        r#"
model Account {
  id Int @id
  balance Decimal

  @@audit
}
"#,
    )
    .expect("model with @@audit should parse");

    assert!(
        schema.models[0]
            .attributes
            .iter()
            .any(|a| a.raw == "@@audit"),
        "expected @@audit in attributes",
    );
}

#[test]
fn rejects_audit_with_arguments() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id

  @@audit(level: "full")
}
"#,
    )
    .expect_err("@@audit with args should fail");

    assert!(
        error.to_string().contains("does not take arguments"),
        "error: {error}",
    );
}
