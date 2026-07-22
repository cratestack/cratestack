#![cfg(test)]

use super::Severity;
use super::test_support::{categories, diff};

#[test]
fn model_removal_is_breaking() {
    let prev = r#"
model Account {
  id Int @id
}

model Invoice {
  id Int @id
}
"#;
    let next = r#"
model Account {
  id Int @id
}
"#;
    let result = diff(prev, next);
    assert!(result.has_breaking());
    assert_eq!(
        categories(&result, Severity::Breaking),
        vec!["model_removed"]
    );
}

#[test]
fn new_model_is_additive() {
    let prev = r#"
model Account {
  id Int @id
}
"#;
    let next = r#"
model Account {
  id Int @id
}

model Invoice {
  id Int @id
}
"#;
    let result = diff(prev, next);
    assert!(!result.has_breaking());
    assert_eq!(categories(&result, Severity::Additive), vec!["model_added"]);
}

#[test]
fn field_removal_is_breaking() {
    let prev = r#"
model Account {
  id Int @id
  note String?
}
"#;
    let next = r#"
model Account {
  id Int @id
}
"#;
    let result = diff(prev, next);
    assert!(result.has_breaking());
    assert_eq!(
        categories(&result, Severity::Breaking),
        vec!["field_removed"]
    );
}

#[test]
fn field_retype_is_breaking() {
    let prev = r#"
model Account {
  id Int @id
  balance Int
}
"#;
    let next = r#"
model Account {
  id Int @id
  balance String
}
"#;
    let result = diff(prev, next);
    assert!(result.has_breaking());
    assert_eq!(
        categories(&result, Severity::Breaking),
        vec!["field_retyped"]
    );
}

#[test]
fn new_optional_field_is_additive() {
    let prev = r#"
model Account {
  id Int @id
}
"#;
    let next = r#"
model Account {
  id Int @id
  note String?
}
"#;
    let result = diff(prev, next);
    assert!(!result.has_breaking());
    assert_eq!(categories(&result, Severity::Additive), vec!["field_added"]);
}

#[test]
fn new_required_field_without_default_is_breaking() {
    let prev = r#"
model Account {
  id Int @id
}
"#;
    let next = r#"
model Account {
  id Int @id
  verified Boolean
}
"#;
    let result = diff(prev, next);
    assert!(result.has_breaking());
    assert_eq!(
        categories(&result, Severity::Breaking),
        vec!["field_added_required"]
    );
}

#[test]
fn new_required_field_with_default_is_additive() {
    let prev = r#"
model Account {
  id Int @id
}
"#;
    let next = r#"
model Account {
  id Int @id
  verified Boolean @default(false)
}
"#;
    let result = diff(prev, next);
    assert!(!result.has_breaking());
    assert_eq!(categories(&result, Severity::Additive), vec!["field_added"]);
}

#[test]
fn widening_field_arity_to_optional_is_additive() {
    let prev = r#"
model Account {
  id Int @id
  balance Int
}
"#;
    let next = r#"
model Account {
  id Int @id
  balance Int?
}
"#;
    let result = diff(prev, next);
    assert!(!result.has_breaking());
    assert_eq!(
        categories(&result, Severity::Additive),
        vec!["field_arity_changed"]
    );
}

#[test]
fn narrowing_field_arity_to_required_is_breaking() {
    let prev = r#"
model Account {
  id Int @id
  note String?
}
"#;
    let next = r#"
model Account {
  id Int @id
  note String
}
"#;
    let result = diff(prev, next);
    assert!(result.has_breaking());
    assert_eq!(
        categories(&result, Severity::Breaking),
        vec!["field_arity_changed"]
    );
}
