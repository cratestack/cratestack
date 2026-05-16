#![cfg(test)]

use super::parse_schema;

#[test]
fn accepts_string_validators() {
    parse_schema(
        r#"
model Account {
  id Int @id
  email String @email
  name String @length(min: 1, max: 200)
  currency String @iso4217
  website String @uri
  slug String @regex("^[a-z0-9-]+$")
}
"#,
    )
    .expect("validator-decorated schema should parse");
}

#[test]
fn rejects_length_on_int_field() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id
  count Int @length(min: 1)
}
"#,
    )
    .expect_err("@length on Int should fail");

    assert!(
        error.to_string().contains("only valid on String or Bytes"),
        "error: {error}",
    );
}

#[test]
fn rejects_email_on_int_field() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id
  count Int @email
}
"#,
    )
    .expect_err("@email on Int should fail");

    assert!(
        error.to_string().contains("only valid on String"),
        "error: {error}",
    );
}

#[test]
fn rejects_invalid_regex_pattern() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id
  bad String @regex("[unterminated")
}
"#,
    )
    .expect_err("invalid regex should fail at parse time");

    assert!(
        error.to_string().contains("not a valid regex"),
        "error: {error}",
    );
}

#[test]
fn rejects_length_with_min_above_max() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id
  name String @length(min: 10, max: 5)
}
"#,
    )
    .expect_err("min > max should fail");

    assert!(
        error.to_string().contains("min (10) must be <= max"),
        "error: {error}",
    );
}

#[test]
fn rejects_range_on_string_field() {
    let error = parse_schema(
        r#"
model Account {
  id Int @id
  name String @range(min: 0)
}
"#,
    )
    .expect_err("@range on String should fail");

    assert!(
        error.to_string().contains("only valid on Int or Decimal"),
        "error: {error}",
    );
}

#[test]
fn decimal_field_can_carry_range_validator() {
    parse_schema(
        r#"
model Account {
  id Int @id
  balance Decimal @range(min: 0)
}
"#,
    )
    .expect("@range on Decimal should be accepted at parse time");
}
