#![cfg(test)]

use super::Severity;
use super::test_support::{categories, diff};

#[test]
fn adding_paged_attribute_is_breaking() {
    let prev = r#"
model Transaction {
  id Int @id
}
"#;
    let next = r#"
model Transaction {
  id Int @id

  @@paged
}
"#;
    let result = diff(prev, next);
    assert!(result.has_breaking());
    assert_eq!(
        categories(&result, Severity::Breaking),
        vec!["model_attribute_paged"]
    );
    let message = &result.changes[0].message;
    assert!(message.contains("Transaction[]"), "message: {message}");
    assert!(message.contains("Page<Transaction>"), "message: {message}");
}

#[test]
fn removing_paged_attribute_is_breaking() {
    let prev = r#"
model Transaction {
  id Int @id

  @@paged
}
"#;
    let next = r#"
model Transaction {
  id Int @id
}
"#;
    let result = diff(prev, next);
    assert!(result.has_breaking());
    assert_eq!(
        categories(&result, Severity::Breaking),
        vec!["model_attribute_paged"]
    );
}

#[test]
fn adding_soft_delete_attribute_is_internal_only() {
    let prev = r#"
model Customer {
  id Int @id
}
"#;
    let next = r#"
model Customer {
  id Int @id

  @@soft_delete
}
"#;
    let result = diff(prev, next);
    assert!(!result.has_breaking());
    assert_eq!(
        categories(&result, Severity::Internal),
        vec!["model_attribute_other"]
    );
}

#[test]
fn changing_retain_days_is_internal_only() {
    let prev = r#"
model Customer {
  id Int @id

  @@retain(days: 30)
}
"#;
    let next = r#"
model Customer {
  id Int @id

  @@retain(days: 90)
}
"#;
    let result = diff(prev, next);
    assert!(!result.has_breaking());
    assert_eq!(
        categories(&result, Severity::Internal),
        vec!["model_attribute_other"]
    );
}
