//! Diff coverage for changes to an *existing* table's primary key
//! (`@@id([...])`, issue #536). Unlike every other diff phase, this
//! one does not turn a detected change into `Op`s — see
//! `diff/primary_key.rs`'s module doc for why. `diff`/`diff_projections`
//! instead return `Err(MigrateError::PrimaryKeyChanged)`: loud and
//! specific, naming the table and the before/after key, rather than
//! the silent empty diff this module used to produce.
//!
//! Modeled on the sibling case done right:
//! `composite_unique.rs::reordering_composite_unique_fields_replaces_the_index`.

use super::super::diff;
use super::{schema, with_models};
use crate::error::MigrateError;

fn account_membership_model(id_attribute: &str) -> String {
    format!(
        r#"
model AccountMembership {{
  accountId Int
  subject String
  active Boolean

  {id_attribute}
}}
"#
    )
}

#[test]
fn changing_composite_primary_key_columns_is_rejected() {
    let prev = schema(&with_models(&account_membership_model(
        "@@id([accountId, subject])",
    )));
    let next = schema(&with_models(&account_membership_model(
        "@@id([accountId, active])",
    )));

    let error = diff(&prev, &next)
        .expect_err("a primary-key column-set change must be refused, not silently dropped");
    assert!(
        matches!(&error, MigrateError::PrimaryKeyChanged { table, .. } if table == "account_memberships"),
        "unexpected error: {error:?}"
    );
    let message = error.to_string();
    assert!(
        message.contains("account_memberships"),
        "message: {message}"
    );
    assert!(
        message.contains("account_id, subject"),
        "message: {message}"
    );
    assert!(message.contains("account_id, active"), "message: {message}");
}

/// Column order is part of the constraint's identity, exactly as it
/// is for `@@unique` (see `composite_unique.rs`) — a reorder is a
/// real schema change, not a no-op. The diff engine derives a table's
/// primary-key column order the same way `emit::postgres::tables` /
/// `emit::sqlite::tables` already do when rendering `PRIMARY KEY
/// (...)`: from each column's `primary_key` flag, in the order
/// columns appear on the table — *not* from `@@id([...])`'s literal
/// argument order, which `convert::project_model` already collapses
/// into a `HashSet` before it reaches here (a separate, pre-existing
/// gap in order fidelity, out of scope for this fix). So the reorder
/// this test exercises is the field-*declaration* order of the two
/// key columns, which is what actually changes the emitted DDL.
#[test]
fn reordering_primary_key_field_declarations_is_rejected() {
    let prev = schema(&with_models(
        r#"
model AccountMembership {
  accountId Int
  subject String
  active Boolean

  @@id([accountId, subject])
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model AccountMembership {
  subject String
  accountId Int
  active Boolean

  @@id([accountId, subject])
}
"#,
    ));

    let error = diff(&prev, &next)
        .expect_err("a primary-key column reorder must be refused, not silently dropped");
    assert!(
        matches!(&error, MigrateError::PrimaryKeyChanged { table, .. } if table == "account_memberships"),
        "unexpected error: {error:?}"
    );
    let message = error.to_string();
    assert!(
        message.contains("account_id, subject"),
        "message: {message}"
    );
    assert!(
        message.contains("subject, account_id"),
        "message: {message}"
    );
}

#[test]
fn unchanged_composite_primary_key_produces_no_ops() {
    let source = with_models(&account_membership_model("@@id([accountId, subject])"));
    let s = schema(&source);
    assert!(diff(&s, &s).expect("diff should succeed").is_empty());
}

/// A brand-new table's primary key is a `CreateTable`, not a change
/// to an existing one — no PK-change refusal should ever fire here.
#[test]
fn creating_a_table_with_a_composite_primary_key_is_not_a_primary_key_change() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(&account_membership_model(
        "@@id([accountId, subject])",
    )));
    let ops = diff(&prev, &next).expect("creating a new table must not be refused");
    assert_eq!(ops.len(), 1, "ops: {ops:?}");
}
