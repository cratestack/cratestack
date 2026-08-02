//! Semantic checks for the model-level `@@unique([...])` composite
//! unique constraint (issue #262). The DDL it produces is covered in
//! `cratestack-migrate`; here we only assert that the schema layer
//! accepts well-formed declarations and rejects the ones that would
//! otherwise reach the emitter as an index over a column that does not
//! exist.

#![cfg(test)]

use super::parse_schema;

#[test]
fn accepts_composite_unique_attribute() {
    let schema = parse_schema(
        r#"
model Application {
  id String @id
  tenantId String
  name String
  environment String

  @@unique([tenantId, name, environment])
}
"#,
    )
    .expect("composite @@unique should parse");

    assert!(
        schema.models[0]
            .attributes
            .iter()
            .any(|a| a.raw == "@@unique([tenantId, name, environment])"),
        "attributes: {:?}",
        schema.models[0].attributes,
    );
}

#[test]
fn accepts_several_distinct_composite_uniques() {
    let schema = parse_schema(
        r#"
model Application {
  id String @id
  tenantId String
  name String
  slug String

  @@unique([tenantId, name])
  @@unique([tenantId, slug])
}
"#,
    )
    .expect("two distinct @@unique constraints should parse");

    assert_eq!(schema.models[0].attributes.len(), 2);
}

#[test]
fn rejects_composite_unique_with_single_field() {
    let error = parse_schema(
        r#"
model Application {
  id String @id
  name String

  @@unique([name])
}
"#,
    )
    .expect_err("single-field @@unique should fail");

    assert!(
        error.to_string().contains("at least two fields"),
        "error: {error}",
    );
    assert!(
        error.to_string().contains("field-level `@unique`"),
        "error: {error}",
    );
}

#[test]
fn rejects_bare_composite_unique() {
    let error = parse_schema(
        r#"
model Application {
  id String @id
  name String

  @@unique
}
"#,
    )
    .expect_err("bare @@unique should fail");

    assert!(
        error.to_string().contains("requires a field list"),
        "error: {error}",
    );
}

#[test]
fn rejects_composite_unique_referencing_unknown_field() {
    let error = parse_schema(
        r#"
model Application {
  id String @id
  tenantId String
  name String

  @@unique([tenantId, environment])
}
"#,
    )
    .expect_err("@@unique referencing unknown field should fail");

    assert!(
        error
            .to_string()
            .contains("references unknown field `environment`"),
        "error: {error}",
    );
}

#[test]
fn rejects_composite_unique_field_that_is_a_relation() {
    let error = parse_schema(
        r#"
model Tenant {
  id Int @id
}

model Application {
  id String @id
  tenantId Int
  name String
  tenant Tenant @relation(fields:[tenantId],references:[id])

  @@unique([tenant, name])
}
"#,
    )
    .expect_err("@@unique listing a relation field should fail");

    assert!(
        error
            .to_string()
            .contains("must be a scalar column, not a relation field"),
        "error: {error}",
    );
}

#[test]
fn rejects_duplicate_composite_unique_attribute() {
    let error = parse_schema(
        r#"
model Application {
  id String @id
  tenantId String
  name String

  @@unique([tenantId, name])
  @@unique([tenantId, name])
}
"#,
    )
    .expect_err("the same @@unique twice should fail");

    assert!(
        error.to_string().contains("more than once"),
        "error: {error}",
    );
}

#[test]
fn rejects_composite_unique_listing_a_field_twice() {
    let error = parse_schema(
        r#"
model Application {
  id String @id
  name String

  @@unique([name, name])
}
"#,
    )
    .expect_err("@@unique repeating a field should fail");

    assert!(
        error.to_string().contains("more than once"),
        "error: {error}",
    );
}

#[test]
fn rejects_composite_unique_without_brackets() {
    let error = parse_schema(
        r#"
model Application {
  id String @id
  tenantId String
  name String

  @@unique(tenantId, name)
}
"#,
    )
    .expect_err("@@unique without brackets should fail");

    assert!(
        error.to_string().contains("must list fields as"),
        "error: {error}",
    );
}
