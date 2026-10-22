use super::super::emit;
use super::{schema, with_models};
use crate::diff::diff;

#[test]
fn add_column_emits_alter_table() {
    let prev = schema(&with_models(
        r#"
model Account {
  id Int @id
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model Account {
  id Int @id
  balance Int?
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration
            .up
            .contains("ALTER TABLE accounts ADD COLUMN balance BIGINT")
    );
    assert!(
        migration
            .down
            .contains("ALTER TABLE accounts DROP COLUMN balance;")
    );
}

#[test]
fn lossy_migration_emits_error_stub_for_down() {
    let prev = schema(&with_models(
        r#"
model Account {
  id Int @id
  legacy String?
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model Account {
  id Int @id
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(migration.has_lossy);
    assert!(
        migration
            .up
            .contains("ALTER TABLE accounts DROP COLUMN legacy;")
    );
    assert!(migration.down.contains("destructive migration"));
    assert!(migration.down.contains("DropColumn accounts.legacy"));
    assert!(!migration.down.contains("ADD COLUMN"));
}

#[test]
fn loosening_required_to_optional_is_safe() {
    let prev = schema(&with_models(
        r#"
model Account {
  id Int @id
  status String
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model Account {
  id Int @id
  status String?
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(!migration.has_lossy);
    assert!(!migration.has_blocking);
    assert!(
        migration
            .up
            .contains("ALTER TABLE accounts ALTER COLUMN status DROP NOT NULL;"),
        "up was: {}",
        migration.up
    );
    assert!(
        migration
            .down
            .contains("ALTER TABLE accounts ALTER COLUMN status SET NOT NULL;"),
        "down was: {}",
        migration.down
    );
}

#[test]
fn tightening_optional_to_required_is_blocking() {
    let prev = schema(&with_models(
        r#"
model Account {
  id Int @id
  status String?
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model Account {
  id Int @id
  status String
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(migration.has_blocking);
    assert!(
        migration
            .up
            .contains("ALTER TABLE accounts ALTER COLUMN status SET NOT NULL;")
    );
    assert!(migration.up.contains("WARNING"));
}

#[test]
fn type_change_is_lossy_and_uses_using_cast() {
    let prev = schema(&with_models(
        r#"
model Account {
  id Int @id
  amount Int
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model Account {
  id Int @id
  amount Decimal
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(migration.has_lossy);
    assert!(
        migration.up.contains(
            "ALTER TABLE accounts ALTER COLUMN amount TYPE NUMERIC USING (amount::NUMERIC);"
        ),
        "up was: {}",
        migration.up
    );
    assert!(migration.down.contains("destructive migration"));
}

#[test]
fn default_change_emits_set_and_drop_default() {
    let prev = schema(&with_models(
        r#"
model Order {
  id Int @id
  status String @default('pending')
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model Order {
  id Int @id
  status String @default('submitted')
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(!migration.has_lossy);
    assert!(
        migration
            .up
            .contains("ALTER TABLE orders ALTER COLUMN status SET DEFAULT 'submitted';"),
        "up was: {}",
        migration.up
    );
    assert!(
        migration
            .down
            .contains("ALTER TABLE orders ALTER COLUMN status SET DEFAULT 'pending';")
    );
}

#[test]
fn dropping_default_emits_drop_default() {
    let prev = schema(&with_models(
        r#"
model Order {
  id Int @id
  status String @default('pending')
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model Order {
  id Int @id
  status String
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration
            .up
            .contains("ALTER TABLE orders ALTER COLUMN status DROP DEFAULT;"),
        "up was: {}",
        migration.up
    );
}

/// Was `switching_to_dbgenerated_emits_drop_default_not_literal`, which
/// asserted the opposite. `DROP DEFAULT` here destroys the very thing
/// `@default(dbgenerated())` asserts exists — a database-level default
/// supplied by a trigger, IDENTITY, or hand-authored DDL — leaving a
/// column the schema says has a default with none at all, so any
/// `INSERT` omitting it starts failing on NOT NULL (cratestack#843).
#[test]
fn switching_to_dbgenerated_emits_no_ddl_and_never_the_literal() {
    let prev = schema(&with_models(
        r#"
model Order {
  id Int @id
  status String @default('pending')
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model Order {
  id Int @id
  status String @default(dbgenerated())
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    // `dbgenerated()` is a cratestack marker, not SQL — Postgres has no
    // such function. It may be *named* in a comment (this emitter now
    // does exactly that), but it must never reach executable DDL.
    for line in migration.up.lines() {
        assert!(
            !line.contains("dbgenerated") || line.trim_start().starts_with("--"),
            "`dbgenerated()` leaked into executable SQL: {line}"
        );
    }
    assert!(
        !migration.up.contains("DROP DEFAULT"),
        "up must not drop a default cratestack does not manage: {}",
        migration.up
    );
    assert!(
        migration.up.contains("-- orders.status switches to"),
        "the transition should still be recorded for the reader: {}",
        migration.up
    );
}

/// The reverse direction must be a true inverse. `down` is generated by
/// swapping `from`/`to` and re-running the same emitter, so an
/// unconditional `DROP DEFAULT` on `to: None` would make the reversal
/// of the no-op above destroy the external default anyway — undoing the
/// fix through the back door.
#[test]
fn reversing_a_switch_to_dbgenerated_does_not_drop_the_external_default() {
    let prev = schema(&with_models(
        r#"
model Order {
  id Int @id
  status String
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model Order {
  id Int @id
  status String @default(dbgenerated())
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        !migration.up.contains("DROP DEFAULT"),
        "up was: {}",
        migration.up
    );
    assert!(
        !migration.down.contains("DROP DEFAULT"),
        "down must not drop a default up never set: {}",
        migration.down
    );
}

/// Unchanged behaviour, pinned so the narrowing above stays narrow:
/// a default cratestack *did* set is still cratestack's to remove.
#[test]
fn dropping_a_managed_literal_default_still_emits_drop_default() {
    let prev = schema(&with_models(
        r#"
model Order {
  id Int @id
  status String @default('pending')
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model Order {
  id Int @id
  status String
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration
            .up
            .contains("ALTER TABLE orders ALTER COLUMN status DROP DEFAULT;"),
        "up was: {}",
        migration.up
    );
}

#[test]
fn switching_from_dbgenerated_emits_real_set_default() {
    let prev = schema(&with_models(
        r#"
model Order {
  id Int @id
  status String @default(dbgenerated())
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model Order {
  id Int @id
  status String @default('pending')
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration
            .up
            .contains("ALTER TABLE orders ALTER COLUMN status SET DEFAULT 'pending';"),
        "up was: {}",
        migration.up
    );
}
