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
    let migration = emit(&diff(&prev, &next));
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
    let migration = emit(&diff(&prev, &next));
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
    let migration = emit(&diff(&prev, &next));
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
    let migration = emit(&diff(&prev, &next));
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
    let migration = emit(&diff(&prev, &next));
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
    let migration = emit(&diff(&prev, &next));
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
    let migration = emit(&diff(&prev, &next));
    assert!(
        migration
            .up
            .contains("ALTER TABLE orders ALTER COLUMN status DROP DEFAULT;"),
        "up was: {}",
        migration.up
    );
}

#[test]
fn switching_to_dbgenerated_emits_drop_default_not_literal() {
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
    let migration = emit(&diff(&prev, &next));
    assert!(
        !migration.up.contains("dbgenerated"),
        "up must never contain the literal `dbgenerated()` call: {}",
        migration.up
    );
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
    let migration = emit(&diff(&prev, &next));
    assert!(
        migration
            .up
            .contains("ALTER TABLE orders ALTER COLUMN status SET DEFAULT 'pending';"),
        "up was: {}",
        migration.up
    );
}
