use super::super::emit;
use super::{schema, with_models};
use crate::diff::diff;

#[test]
fn db_enforce_range_emits_check_constraint() {
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
    // The table is created by this migration, so the CHECK cannot
    // reject any existing row (cratestack#1129).
    assert!(
        !migration.has_blocking,
        "a CHECK on a table this migration creates cannot block; up was: {}",
        migration.up
    );
    assert!(
        migration.up_pre.is_none(),
        "no up.pre.sql for a first migration"
    );
    assert!(
        !migration.up.contains("WARNING"),
        "up was: {}",
        migration.up
    );
    assert!(
        migration.up.contains(
            "ALTER TABLE members ADD CONSTRAINT members_amount_range_check \
             CHECK (amount >= 0 AND amount <= 1000000);"
        ),
        "up was: {}",
        migration.up
    );
}

/// The case the blocking warning exists for (cratestack#843): the table
/// already exists, so `@db_enforce` promotes a CHECK onto rows that may
/// violate it.
#[test]
fn db_enforce_on_existing_table_is_blocking_and_scaffolds_up_pre() {
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
    let up_pre = migration
        .up_pre
        .as_deref()
        .expect("up.pre.sql is scaffolded for an existing table");
    assert!(up_pre.contains("members.amount"), "up_pre was: {up_pre}");
    assert!(
        up_pre.contains("members_amount_range_check"),
        "up_pre was: {up_pre}"
    );
    assert!(migration.up.contains("WARNING"), "up was: {}", migration.up);
}

#[test]
fn db_enforce_iso4217_uses_regex_predicate() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Member {
  id Int @id
  currency String @iso4217 @db_enforce
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration.up.contains("CHECK (currency ~ '^[A-Z]{3}$')"),
        "up was: {}",
        migration.up
    );
}

#[test]
fn db_enforce_length_emits_length_predicate() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Member {
  id Int @id
  email String @length(min: 3, max: 254) @db_enforce
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration
            .up
            .contains("CHECK (length(email) BETWEEN 3 AND 254)"),
        "up was: {}",
        migration.up
    );
}

#[test]
fn removing_db_enforce_drops_constraint() {
    let prev = schema(&with_models(
        r#"
model Member {
  id Int @id
  amount Int @range(min: 0) @db_enforce
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model Member {
  id Int @id
  amount Int @range(min: 0)
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(!migration.has_blocking);
    assert!(!migration.has_lossy);
    assert!(
        migration
            .up
            .contains("ALTER TABLE members DROP CONSTRAINT members_amount_range_check;"),
        "up was: {}",
        migration.up
    );
}
