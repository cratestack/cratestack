//! Tests for the general validation entry — required/optional/null
//! handling and JSON type matching. Predicate-specific tests live in
//! [`super::predicate_tests`].

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

const POST: &str = r#"
    model Post {
      id String @id
      title String
    }
"#;

const POST_OPTIONAL_BODY: &str = r#"
    model Post {
      id String @id
      title String
      body String?
    }
"#;

#[test]
fn required_field_missing_on_create_errors() {
    let schema = parse(POST_OPTIONAL_BODY);
    let model = schema.models.iter().find(|m| m.name == "Post").unwrap();
    let errors = validate_payload(model, &payload(&[("id", serde_json::json!("a"))]), false);
    let codes: Vec<ValidationCode> = errors.iter().map(|e| e.code).collect();
    assert!(codes.contains(&ValidationCode::Required));
    assert!(!errors.iter().any(|e| e.field == "body"));
}

#[test]
fn required_field_missing_on_update_is_ok() {
    let schema = parse(POST);
    let model = schema.models.iter().find(|m| m.name == "Post").unwrap();
    let errors = validate_payload(model, &payload(&[]), true);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn type_mismatch_flags_wrong_json_type() {
    let schema = parse(POST);
    let model = schema.models.iter().find(|m| m.name == "Post").unwrap();
    let bad = validate_payload(
        model,
        &payload(&[("id", serde_json::json!("p")), ("title", serde_json::json!(42))]),
        false,
    );
    assert!(bad.iter().any(|e| e.code == ValidationCode::TypeMismatch));
}

#[test]
fn null_on_required_field_errors_but_null_on_optional_ok() {
    let schema = parse(POST_OPTIONAL_BODY);
    let model = schema.models.iter().find(|m| m.name == "Post").unwrap();
    let bad = validate_payload(
        model,
        &payload(&[
            ("id", serde_json::json!("p")),
            ("title", serde_json::json!(null)),
            ("body", serde_json::json!(null)),
        ]),
        false,
    );
    let title_err = bad
        .iter()
        .any(|e| e.field == "title" && e.code == ValidationCode::Required);
    let body_err = bad.iter().any(|e| e.field == "body");
    assert!(title_err);
    assert!(!body_err);
}
