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

#[test]
fn each_composite_unique_is_tracked_separately() {
    // Keyed on the whole attribute, not just `@@unique`: dropping one
    // of two constraints has to read as a removal, not vanish because
    // the surviving one occupies the same key (issue #262).
    let prev = r#"
model Application {
  id String @id
  tenantId String
  name String
  slug String

  @@unique([tenantId, name])
  @@unique([tenantId, slug])
}
"#;
    let next = r#"
model Application {
  id String @id
  tenantId String
  name String
  slug String

  @@unique([tenantId, name])
}
"#;
    let result = diff(prev, next);
    assert!(!result.has_breaking());
    assert_eq!(
        categories(&result, Severity::Internal),
        vec!["model_attribute_other"]
    );
    let message = &result.changes[0].message;
    assert!(
        message.contains("lost `@@unique([tenantId, slug])`"),
        "message: {message}"
    );
}

#[test]
fn adding_a_composite_unique_is_internal_only() {
    let prev = r#"
model Application {
  id String @id
  tenantId String
  name String
}
"#;
    let next = r#"
model Application {
  id String @id
  tenantId String
  name String

  @@unique([tenantId, name])
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
fn composite_unique_whitespace_only_edit_is_not_reported_as_add_and_remove() {
    // `Attribute::raw` preserves the source line verbatim, so a purely
    // cosmetic reflow of the field list (e.g. running a formatter) must
    // not surface as a constraint being dropped and a different one
    // added — the key has to be blind to whitespace even though `@@id`
    // and friends aren't (they key on the bare attribute name instead).
    let prev = r#"
model Application {
  id String @id
  tenantId String
  name String

  @@unique([tenantId, name])
}
"#;
    let next = r#"
model Application {
  id String @id
  tenantId String
  name String

  @@unique([tenantId,name])
}
"#;
    let result = diff(prev, next);
    assert!(!result.has_breaking());
    // Whitespace-only: same shape the codebase already accepts for
    // `@@retain(days: N)` picking up a literal-text difference —  a
    // single `Changed` entry, never a spurious remove+add pair.
    assert_eq!(
        categories(&result, Severity::Internal),
        vec!["model_attribute_other"]
    );
}
