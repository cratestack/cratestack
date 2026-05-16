use super::super::emit;
use super::{schema, with_models};
use crate::diff::diff;

#[test]
fn table_rename_emits_alter_table_rename_to() {
    let prev = schema(&with_models(
        r#"
model OldName {
  id Int @id
  label String
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model NewName {
  id Int @id
  label String

  @@rename(from = "old_names")
}
"#,
    ));
    let migration = emit(&diff(&prev, &next));
    assert!(!migration.has_lossy, "up was: {}", migration.up);
    assert!(
        migration
            .up
            .contains("ALTER TABLE old_names RENAME TO new_names;"),
        "up was: {}",
        migration.up
    );
    // No drop/add — the table was renamed, not recreated.
    assert!(!migration.up.contains("DROP TABLE"));
    assert!(!migration.up.contains("CREATE TABLE"));
    assert!(
        migration
            .down
            .contains("ALTER TABLE new_names RENAME TO old_names;"),
        "down was: {}",
        migration.down
    );
}

#[test]
fn column_rename_emits_alter_table_rename_column() {
    let prev = schema(&with_models(
        r#"
model Customer {
  id Int @id
  email String
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model Customer {
  id Int @id
  emailAddress String @rename(from = "email")
}
"#,
    ));
    let migration = emit(&diff(&prev, &next));
    assert!(!migration.has_lossy);
    assert!(
        migration
            .up
            .contains("ALTER TABLE customers RENAME COLUMN email TO email_address;"),
        "up was: {}",
        migration.up
    );
    // No drop/add — the column was renamed, not recreated.
    assert!(!migration.up.contains("DROP COLUMN"));
    assert!(!migration.up.contains("ADD COLUMN"));
}

#[test]
fn rename_without_matching_old_falls_back_to_add() {
    // A @rename(from = "doesnt_exist") on a brand-new column can't
    // match an existing column — the diff engine falls back to
    // AddColumn and ignores the rename marker.
    let prev = schema(&with_models(r#"
model Customer {
  id Int @id
}
"#));
    let next = schema(&with_models(
        r#"
model Customer {
  id Int @id
  emailAddress String? @rename(from = "nope")
}
"#,
    ));
    let migration = emit(&diff(&prev, &next));
    assert!(
        migration
            .up
            .contains("ALTER TABLE customers ADD COLUMN email_address TEXT;"),
        "up was: {}",
        migration.up
    );
    assert!(!migration.up.contains("RENAME COLUMN"));
}
