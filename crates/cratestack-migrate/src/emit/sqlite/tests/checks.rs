//! Validator CHECKs on SQLite (cratestack#1129). SQLite never scaffolds
//! `up.pre.sql`, so the observable effects are `has_blocking` (the flag
//! the CLI prints) and the `up.sql` WARNING header.

use super::super::emit;
use super::{schema, with_models};
use crate::diff::diff;

/// A first migration: the table is created here, so the `@db_enforce`
/// CHECK cannot reject existing rows.
#[test]
fn validator_check_on_table_created_in_same_migration_is_not_blocking() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Member {
  id Int @id
  amount Int @range(min: 0, max: 1000000) @db_enforce
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(!migration.has_blocking, "up was: {}", migration.up);
    assert!(
        !migration.up.contains("WARNING"),
        "up was: {}",
        migration.up
    );
    assert!(migration.up_pre.is_none());
}

/// The table exists already, so promoting `@db_enforce` onto it is
/// blocking, with the per-op reason in the WARNING.
#[test]
fn db_enforce_on_existing_table_is_blocking_with_warning() {
    let prev = schema(&with_models(
        r#"
model Member {
  id Int @id
  amount Int @range(min: 0, max: 1000000)
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model Member {
  id Int @id
  amount Int @range(min: 0, max: 1000000) @db_enforce
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(migration.has_blocking, "up was: {}", migration.up);
    assert!(migration.up.contains("WARNING"), "up was: {}", migration.up);
    assert!(
        migration.up.contains("members.amount"),
        "up was: {}",
        migration.up
    );
}
