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
