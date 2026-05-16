#![cfg(test)]

use super::parse_schema;

#[test]
fn parses_version_attribute_on_int_field() {
    let schema = parse_schema(
        r#"
model Account {
  id Int @id
  balance Int
  version Int @version
}
"#,
    )
    .expect("schema with @version should parse");

    let version_field = &schema.models[0].fields[2];
    assert_eq!(version_field.name, "version");
    assert!(
        version_field.attributes.iter().any(|a| a.raw == "@version"),
        "@version attribute should be present"
    );
}

#[test]
fn rejects_two_version_fields_per_model() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id
  v1 Int @version
  v2 Int @version
}
"#,
    )
    .expect_err("two @version fields should fail");

    assert!(
        error.to_string().contains("more than one @version"),
        "error message mentions duplicate @version: {error}",
    );
}

#[test]
fn rejects_version_on_non_int_field() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id
  version String @version
}
"#,
    )
    .expect_err("@version on String should fail");

    assert!(
        error.to_string().contains("must be a required `Int`"),
        "error message mentions Int requirement: {error}",
    );
}

#[test]
fn rejects_version_on_optional_int() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id
  version Int? @version
}
"#,
    )
    .expect_err("@version on Int? should fail");

    assert!(
        error.to_string().contains("must be a required `Int`"),
        "error message mentions Int requirement: {error}",
    );
}

#[test]
fn rejects_version_on_primary_key() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id @version
}
"#,
    )
    .expect_err("@version on @id should fail");

    assert!(
        error
            .to_string()
            .contains("must not also be the primary key"),
        "error message: {error}",
    );
}
