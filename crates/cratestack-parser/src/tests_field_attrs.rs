#![cfg(test)]

use super::parse_schema;

#[test]
fn accepts_readonly_and_server_only_field_attributes() {
    let schema = parse_schema(
        r#"
model Account {
  id Int @id
  balance Decimal @readonly
  internalScore Int @server_only
}
"#,
    )
    .expect("schema with field-policy attributes should parse");

    let fields = &schema.models[0].fields;
    assert!(
        fields[1].attributes.iter().any(|a| a.raw == "@readonly"),
        "expected @readonly on balance",
    );
    assert!(
        fields[2].attributes.iter().any(|a| a.raw == "@server_only"),
        "expected @server_only on internalScore",
    );
}

#[test]
fn rejects_readonly_on_primary_key() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id @readonly
}
"#,
    )
    .expect_err("@readonly on @id should fail");

    assert!(
        error
            .to_string()
            .contains("primary key and must not declare @readonly"),
        "error: {error}",
    );
}

#[test]
fn rejects_server_only_on_primary_key() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id @server_only
}
"#,
    )
    .expect_err("@server_only on @id should fail");

    assert!(
        error
            .to_string()
            .contains("primary key and must not declare @server_only"),
        "error: {error}",
    );
}

#[test]
fn rejects_readonly_and_server_only_together() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id
  balance Decimal @readonly @server_only
}
"#,
    )
    .expect_err("combining @readonly + @server_only should fail");

    assert!(
        error
            .to_string()
            .contains("declares both @readonly and @server_only"),
        "error: {error}",
    );
}

#[test]
fn accepts_bare_dbgenerated_default() {
    let schema = parse_schema(
        r#"
model Article {
  id String @id @default(dbgenerated())
  createdAt DateTime @default(dbgenerated())
}
"#,
    )
    .expect("bare @default(dbgenerated()) should parse");

    let fields = &schema.models[0].fields;
    assert!(
        fields[0]
            .attributes
            .iter()
            .any(|a| a.raw == "@default(dbgenerated())"),
    );
}

#[test]
fn rejects_dbgenerated_with_argument() {
    let error = parse_schema(
        r#"
model Article {
  id String @id @default(dbgenerated("gen_random_uuid()"))
}
"#,
    )
    .expect_err("dbgenerated() with an argument should fail");

    assert!(
        error.to_string().contains("takes no argument"),
        "error: {error}",
    );
}

#[test]
fn accepts_pii_and_sensitive_field_attributes() {
    let schema = parse_schema(
        r#"
model Customer {
  id Int @id
  email String @pii
  riskScore Int @sensitive
}
"#,
    )
    .expect("schema with @pii and @sensitive should parse");

    let fields = &schema.models[0].fields;
    assert!(fields[1].attributes.iter().any(|a| a.raw == "@pii"));
    assert!(fields[2].attributes.iter().any(|a| a.raw == "@sensitive"));
}

#[test]
fn accepts_pb_field_attribute() {
    let schema = parse_schema(
        r#"
model User {
  id Int @id
  email String @pb(5)
}
"#,
    )
    .expect("schema with @pb should parse");

    let fields = &schema.models[0].fields;
    assert!(fields[1].attributes.iter().any(|a| a.raw == "@pb(5)"));
}

#[test]
fn rejects_duplicate_pb_attribute() {
    let error = parse_schema(
        r#"
model User {
  id Int @id
  email String @pb(5) @pb(6)
}
"#,
    )
    .expect_err("duplicate @pb should fail");

    assert!(
        error.to_string().contains("declares `@pb` more than once"),
        "error: {error}",
    );
}

#[test]
fn rejects_pb_with_no_args() {
    let error = parse_schema(
        r#"
model User {
  id Int @id
  email String @pb
}
"#,
    )
    .expect_err("@pb with no args should fail");

    assert!(
        error.to_string().contains("invalid `@pb` attribute"),
        "error: {error}",
    );
}

#[test]
fn rejects_pb_with_non_numeric_arg() {
    let error = parse_schema(
        r#"
model User {
  id Int @id
  email String @pb(abc)
}
"#,
    )
    .expect_err("@pb(abc) should fail");

    assert!(
        error
            .to_string()
            .contains("expected a non-negative integer"),
        "error: {error}",
    );
}

#[test]
fn rejects_pb_in_protobuf_reserved_range() {
    let error = parse_schema(
        r#"
model User {
  id Int @id
  email String @pb(19500)
}
"#,
    )
    .expect_err("@pb(19500) should fail");

    assert!(
        error
            .to_string()
            .contains("protobuf's own reserved field-number range"),
        "error: {error}",
    );
}
