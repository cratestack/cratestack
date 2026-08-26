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
fn adding_internal_attribute_is_breaking() {
    // cratestack#743 (`docs/design/route-suppression.md`): suppressing
    // a live REST route / RPC dispatch arm / client stub is a real
    // break for any consumer still calling it — `cratestack diff` must
    // gate CI on it, not fall through to the generic "no tracked
    // wire-shape effect" branch every other model attribute gets.
    let prev = r#"
model Widget {
  id Int @id
}
"#;
    let next = r#"
model Widget {
  id Int @id

  @@internal("create")
}
"#;
    let result = diff(prev, next);
    assert!(result.has_breaking());
    assert_eq!(
        categories(&result, Severity::Breaking),
        vec!["model_attribute_internal"]
    );
    let message = &result.changes[0].message;
    assert!(
        message.contains("@@internal(\"create\")"),
        "message: {message}"
    );
}

#[test]
fn removing_internal_attribute_is_additive() {
    let prev = r#"
model Widget {
  id Int @id

  @@internal("create")
}
"#;
    let next = r#"
model Widget {
  id Int @id
}
"#;
    let result = diff(prev, next);
    assert!(!result.has_breaking());
    assert_eq!(
        categories(&result, Severity::Additive),
        vec!["model_attribute_internal"]
    );
}

#[test]
fn swapping_the_suppressed_action_reports_both_a_removal_and_an_addition() {
    // `attribute_key` now bakes the action name into the map key
    // (cratestack#743 post-merge review, Finding A — see its doc on
    // `attribute_key`/`push_attribute_change`), so replacing
    // `@@internal("create")` with `@@internal("update")` is NOT a
    // same-key `Changed` entry — it's a distinct-key `Removed` (create
    // restored, Additive) paired with a distinct-key `Added` (update
    // newly suppressed, Breaking). Both must be reported; reporting only
    // one would silently under-count exactly the kind of change this
    // tool exists to catch.
    let prev = r#"
model Widget {
  id Int @id

  @@internal("create")
}
"#;
    let next = r#"
model Widget {
  id Int @id

  @@internal("update")
}
"#;
    let result = diff(prev, next);
    assert!(result.has_breaking());
    assert_eq!(
        categories(&result, Severity::Breaking),
        vec!["model_attribute_internal"]
    );
    assert_eq!(
        categories(&result, Severity::Additive),
        vec!["model_attribute_internal"]
    );
    let messages: Vec<&str> = result.changes.iter().map(|c| c.message.as_str()).collect();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("gained `@@internal(\"update\")`")),
        "messages: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("lost `@@internal(\"create\")`")),
        "messages: {messages:?}"
    );
}

#[test]
fn whitespace_only_edit_of_an_internal_attribute_is_internal_only() {
    // Mirrors `composite_unique_whitespace_only_edit_is_not_reported_as_
    // add_and_remove` below: a purely cosmetic reflow must land as a
    // single same-key `Changed` entry, not a spurious remove+add pair,
    // and — because the action name is unambiguous here (whitespace is
    // the only difference) — it must be `Internal`, not `Breaking`.
    let prev = r#"
model Widget {
  id Int @id

  @@internal("create")
}
"#;
    let next = r#"
model Widget {
  id Int @id

  @@internal( "create" )
}
"#;
    let result = diff(prev, next);
    assert!(!result.has_breaking());
    assert_eq!(
        categories(&result, Severity::Internal),
        vec!["model_attribute_internal"]
    );
}

#[test]
fn adding_two_suppressed_actions_together_reports_both_not_just_the_last() {
    // cratestack#743 post-merge review, Finding A, scenario 1: before
    // the `attribute_key` fix, `@@internal("create")` and
    // `@@internal("update")` both keyed to the bare string `@@internal`,
    // so `index_attributes`' `BTreeMap` silently kept only the last one
    // written — this reported ONE change (`update`), not two.
    let prev = r#"
model Widget {
  id Int @id
}
"#;
    let next = r#"
model Widget {
  id Int @id

  @@internal("create")
  @@internal("update")
}
"#;
    let result = diff(prev, next);
    let mut breaking = categories(&result, Severity::Breaking);
    breaking.sort_unstable();
    assert_eq!(
        breaking,
        vec!["model_attribute_internal", "model_attribute_internal"],
        "both newly suppressed actions must be reported as separate Breaking changes"
    );
    let messages: Vec<&str> = result.changes.iter().map(|c| c.message.as_str()).collect();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("gained `@@internal(\"create\")`")),
        "create's suppression must not be swallowed by update's: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("gained `@@internal(\"update\")`")),
        "messages: {messages:?}"
    );
}

#[test]
fn restoring_one_of_two_suppressed_actions_is_not_silently_dropped() {
    // cratestack#743 post-merge review, Finding A, scenario 2 — the
    // critical one: before the fix, going from `{@@internal("create"),
    // @@internal("update")}` to `{@@internal("update")}` reported ZERO
    // changes, because both prev-side declarations collapsed onto the
    // single `@@internal` key and the surviving `update` entry compared
    // equal to itself. That is exactly the defect this whole
    // classification exists to prevent: a PR that restores a suppressed
    // live action (`create`) passing the diff gate completely unnoticed.
    let prev = r#"
model Widget {
  id Int @id

  @@internal("create")
  @@internal("update")
}
"#;
    let next = r#"
model Widget {
  id Int @id

  @@internal("update")
}
"#;
    let result = diff(prev, next);
    assert!(
        !result.changes.is_empty(),
        "restoring `create` while `update` stays suppressed must be reported, not silently \
         dropped"
    );
    assert_eq!(
        categories(&result, Severity::Additive),
        vec!["model_attribute_internal"]
    );
    assert!(!result.has_breaking());
    let message = &result.changes[0].message;
    assert!(
        message.contains("lost `@@internal(\"create\")`"),
        "message: {message}"
    );
}

#[test]
fn suppressed_action_order_in_source_does_not_affect_the_diff() {
    // cratestack#743 post-merge review, Finding B: two schemas declaring
    // the same two suppressed actions in a different source order must
    // diff as no changes at all — order must not matter.
    let prev = r#"
model Widget {
  id Int @id

  @@internal("create")
  @@internal("update")
}
"#;
    let next = r#"
model Widget {
  id Int @id

  @@internal("update")
  @@internal("create")
}
"#;
    let result = diff(prev, next);
    assert!(
        result.changes.is_empty(),
        "reordering the same two @@internal(...) declarations must not diff as a change: {:?}",
        result.changes
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
