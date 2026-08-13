//! Semantic checks for the model-level `@@index([...], using: ...,
//! opclass: "...")` attribute (cratestack#156). The DDL it produces is
//! covered in `cratestack-migrate`; here we only assert that the schema
//! layer accepts well-formed declarations (bare, `using`-only, and
//! `using`+`opclass`) and rejects the ones that would otherwise reach the
//! emitter as an index over a column that does not exist.

#![cfg(test)]

use super::parse_schema;

#[test]
fn accepts_bare_index_attribute() {
    let schema = parse_schema(
        r#"
model Order {
  id String @id
  customerEmail String

  @@index([customerEmail])
}
"#,
    )
    .expect("bare @@index should parse");

    assert!(
        schema.models[0]
            .attributes
            .iter()
            .any(|a| a.raw == "@@index([customerEmail])"),
        "attributes: {:?}",
        schema.models[0].attributes,
    );
}

#[test]
fn accepts_index_attribute_with_using_and_opclass() {
    let schema = parse_schema(
        r#"
model Document {
  id String @id
  body String

  @@index([body], using: ivfflat, opclass: "vector_l2_ops")
}
"#,
    )
    .expect("@@index with using/opclass should parse");

    assert!(
        schema.models[0]
            .attributes
            .iter()
            .any(|a| a.raw == "@@index([body], using: ivfflat, opclass: \"vector_l2_ops\")"),
        "attributes: {:?}",
        schema.models[0].attributes,
    );
}

#[test]
fn accepts_index_attribute_with_using_only() {
    let schema = parse_schema(
        r#"
model Document {
  id String @id
  body String

  @@index([body], using: gin)
}
"#,
    )
    .expect("@@index with using only should parse");

    assert_eq!(schema.models[0].attributes.len(), 1);
}

#[test]
fn accepts_bare_and_specialized_index_on_the_same_field() {
    let schema = parse_schema(
        r#"
model Document {
  id String @id
  body String

  @@index([body])
  @@index([body], using: gin)
}
"#,
    )
    .expect("a bare index and a specialized index over the same field are not a collision");

    assert_eq!(schema.models[0].attributes.len(), 2);
}

#[test]
fn rejects_bare_index_without_field_list() {
    let error = parse_schema(
        r#"
model Order {
  id String @id
  name String

  @@index
}
"#,
    )
    .expect_err("@@index with no field list should fail");

    assert!(
        error.to_string().contains("requires a field list"),
        "error: {error}",
    );
}

#[test]
fn rejects_index_referencing_unknown_field() {
    let error = parse_schema(
        r#"
model Order {
  id String @id
  name String

  @@index([missing])
}
"#,
    )
    .expect_err("@@index referencing unknown field should fail");

    assert!(
        error
            .to_string()
            .contains("references unknown field `missing`"),
        "error: {error}",
    );
}

#[test]
fn rejects_index_field_that_is_a_relation() {
    let error = parse_schema(
        r#"
model Tenant {
  id Int @id
}

model Application {
  id String @id
  tenantId Int
  tenant Tenant @relation(fields:[tenantId],references:[id])

  @@index([tenant])
}
"#,
    )
    .expect_err("@@index listing a relation field should fail");

    assert!(
        error
            .to_string()
            .contains("must be a scalar column, not a relation field"),
        "error: {error}",
    );
}

#[test]
fn rejects_duplicate_index_attribute() {
    let error = parse_schema(
        r#"
model Order {
  id String @id
  name String

  @@index([name])
  @@index([name])
}
"#,
    )
    .expect_err("the same @@index twice should fail");

    assert!(
        error.to_string().contains("more than once"),
        "error: {error}",
    );
}

#[test]
fn rejects_duplicate_index_attribute_with_same_using() {
    let error = parse_schema(
        r#"
model Document {
  id String @id
  body String

  @@index([body], using: gin)
  @@index([body], using: gin)
}
"#,
    )
    .expect_err("the same @@index([...], using: ...) twice should fail");

    assert!(
        error.to_string().contains("more than once"),
        "error: {error}",
    );
}

#[test]
fn rejects_index_with_invalid_using_value() {
    let error = parse_schema(
        r#"
model Document {
  id String @id
  body String

  @@index([body], using: "gin")
}
"#,
    )
    .expect_err("a quoted `using` value should fail");

    assert!(
        error.to_string().contains("invalid `using` value"),
        "error: {error}",
    );
}

#[test]
fn rejects_index_with_unquoted_opclass_value() {
    let error = parse_schema(
        r#"
model Document {
  id String @id
  body String

  @@index([body], using: ivfflat, opclass: vector_l2_ops)
}
"#,
    )
    .expect_err("an unquoted `opclass` value should fail");

    assert!(
        error.to_string().contains("expected a quoted string"),
        "error: {error}",
    );
}

#[test]
fn does_not_misroute_an_unrelated_attribute_starting_with_index() {
    // Same discipline `@@id(`/`@@unique(` already use (see
    // `tests_model_unique.rs`'s equivalent case): the dispatch guard
    // requires the opening paren (or an exact bare match), so a
    // hypothetical future attribute like `@@index_hint(...)` isn't
    // misrouted into the `@@index` validator.
    let error = parse_schema(
        r#"
model Order {
  id String @id
  name String

  @@index_hint(name)
}
"#,
    );

    if let Err(error) = error {
        assert!(
            !error.to_string().contains("requires a field list"),
            "an unrelated `@@index_hint` attribute must not be misrouted into the @@index \
             validator: {error}",
        );
    }
}
