//! Tests for individual attribute predicates exercised through
//! [`super::validate_payload`]. Kept separate from
//! [`super::tests`] so each file stays inside the 200-LoC budget.

use super::{ValidationCode, validate_payload};

fn parse(text: &str) -> cratestack_core::Schema {
    cratestack_parser::parse_schema(text).expect("schema parses")
}

fn payload(items: &[(&str, serde_json::Value)]) -> serde_json::Map<String, serde_json::Value> {
    items
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect()
}

#[test]
fn email_validator_passes_and_fails() {
    let schema = parse(
        r#"
            model User {
              id String @id
              email String @email
            }
        "#,
    );
    let model = schema.models.iter().find(|m| m.name == "User").unwrap();
    let ok = validate_payload(
        model,
        &payload(&[
            ("id", serde_json::json!("u1")),
            ("email", serde_json::json!("alice@example.com")),
        ]),
        false,
    );
    assert!(ok.is_empty(), "{ok:?}");

    let bad = validate_payload(
        model,
        &payload(&[
            ("id", serde_json::json!("u1")),
            ("email", serde_json::json!("not-an-email")),
        ]),
        false,
    );
    assert!(bad.iter().any(|e| e.code == ValidationCode::Email));
}

#[test]
fn length_validator_enforces_min_and_max() {
    let schema = parse(
        r#"
            model Post {
              id String @id
              title String @length(min: 3, max: 10)
            }
        "#,
    );
    let model = schema.models.iter().find(|m| m.name == "Post").unwrap();
    let short = validate_payload(
        model,
        &payload(&[
            ("id", serde_json::json!("p")),
            ("title", serde_json::json!("hi")),
        ]),
        false,
    );
    assert!(short.iter().any(|e| e.code == ValidationCode::Length));

    let long = validate_payload(
        model,
        &payload(&[
            ("id", serde_json::json!("p")),
            ("title", serde_json::json!("this is too long")),
        ]),
        false,
    );
    assert!(long.iter().any(|e| e.code == ValidationCode::Length));
}

#[test]
fn range_validator_enforces_bounds() {
    let schema = parse(
        r#"
            model Customer {
              id Int @id
              age Int @range(min: 0, max: 120)
            }
        "#,
    );
    let model = schema.models.iter().find(|m| m.name == "Customer").unwrap();
    let bad = validate_payload(
        model,
        &payload(&[("id", serde_json::json!(1)), ("age", serde_json::json!(-1))]),
        false,
    );
    assert!(bad.iter().any(|e| e.code == ValidationCode::Range));
}

#[test]
fn regex_validator_enforces_pattern() {
    let schema = parse(
        r#"
            model Code {
              id String @id
              slug String @regex("^[a-z][a-z0-9-]*$")
            }
        "#,
    );
    let model = schema.models.iter().find(|m| m.name == "Code").unwrap();
    let bad = validate_payload(
        model,
        &payload(&[
            ("id", serde_json::json!("c1")),
            ("slug", serde_json::json!("Bad Slug!")),
        ]),
        false,
    );
    assert!(bad.iter().any(|e| e.code == ValidationCode::Regex));
}

#[test]
fn iso4217_requires_three_uppercase_letters() {
    let schema = parse(
        r#"
            model Account {
              id String @id
              currency String @iso4217
            }
        "#,
    );
    let model = schema.models.iter().find(|m| m.name == "Account").unwrap();
    let ok = validate_payload(
        model,
        &payload(&[
            ("id", serde_json::json!("a1")),
            ("currency", serde_json::json!("USD")),
        ]),
        false,
    );
    assert!(ok.is_empty(), "{ok:?}");

    let bad = validate_payload(
        model,
        &payload(&[
            ("id", serde_json::json!("a1")),
            ("currency", serde_json::json!("usd")),
        ]),
        false,
    );
    assert!(bad.iter().any(|e| e.code == ValidationCode::Iso4217));
}
