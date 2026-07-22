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

#[test]
fn accepts_composite_id_attribute() {
    let schema = parse_schema(
        r#"
model Account {
  id Int @id
}

model AccountMembership {
  accountId Int
  subject String
  active Boolean
  account Account @relation(fields:[accountId],references:[id])

  @@id([accountId, subject])
}
"#,
    )
    .expect("composite @@id should parse");

    let membership = &schema.models[1];
    assert!(
        membership
            .attributes
            .iter()
            .any(|a| a.raw == "@@id([accountId, subject])")
    );
}

#[test]
fn rejects_composite_id_with_single_field() {
    let error = parse_schema(
        r#"
model AccountMembership {
  accountId Int
  subject String

  @@id([accountId])
}
"#,
    )
    .expect_err("single-field @@id should fail");

    assert!(
        error.to_string().contains("at least two fields"),
        "error: {error}",
    );
}

#[test]
fn rejects_composite_id_referencing_unknown_field() {
    let error = parse_schema(
        r#"
model AccountMembership {
  accountId Int
  subject String

  @@id([accountId, role])
}
"#,
    )
    .expect_err("@@id referencing unknown field should fail");

    assert!(
        error
            .to_string()
            .contains("references unknown field `role`"),
        "error: {error}",
    );
}

#[test]
fn rejects_composite_id_alongside_field_level_id() {
    let error = parse_schema(
        r#"
model AccountMembership {
  id Cuid @id
  accountId Int
  subject String

  @@id([accountId, subject])
}
"#,
    )
    .expect_err("field-level @id plus @@id should fail");

    assert!(
        error
            .to_string()
            .contains("use exactly one primary key declaration"),
        "error: {error}",
    );
}

#[test]
fn rejects_duplicate_composite_id_attribute() {
    let error = parse_schema(
        r#"
model AccountMembership {
  accountId Int
  subject String

  @@id([accountId, subject])
  @@id([accountId, subject])
}
"#,
    )
    .expect_err("duplicate @@id attributes should fail");

    assert!(
        error
            .to_string()
            .contains("must not declare more than one @@id(...) attribute"),
        "error: {error}",
    );
}

#[test]
fn rejects_composite_id_field_that_is_a_relation() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id
}

model AccountMembership {
  accountId Int
  subject String
  account Account @relation(fields:[accountId],references:[id])

  @@id([account, subject])
}
"#,
    )
    .expect_err("@@id listing a relation field should fail");

    assert!(
        error
            .to_string()
            .contains("must be a scalar column, not a relation field"),
        "error: {error}",
    );
}

#[test]
fn rejects_composite_id_field_with_readonly() {
    let error = parse_schema(
        r#"
model AccountMembership {
  accountId Int @readonly
  subject String

  @@id([accountId, subject])
}
"#,
    )
    .expect_err("@@id field carrying @readonly should fail");

    assert!(
        error
            .to_string()
            .contains("must not declare @readonly or @server_only"),
        "error: {error}",
    );
}

#[test]
fn model_missing_primary_key_error_mentions_composite_form() {
    let error = parse_schema(
        r#"
model AccountMembership {
  accountId Int
  subject String
}
"#,
    )
    .expect_err("model without any primary key should fail");

    assert!(error.to_string().contains("@@id([...])"), "error: {error}",);
}
