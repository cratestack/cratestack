use super::super::emit;
use super::{schema, with_models};
use crate::diff::diff;

#[test]
fn create_table_emits_postgres_ddl() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Account {
  id Int @id
  balance Int
  note String?
}
"#,
    ));
    let migration = emit(&diff(&prev, &next));
    assert!(!migration.has_lossy);
    assert!(!migration.has_blocking);
    assert!(
        migration.up.contains("CREATE TABLE accounts"),
        "up was: {}",
        migration.up
    );
    assert!(migration.up.contains("id BIGINT NOT NULL"));
    assert!(migration.up.contains("balance BIGINT NOT NULL"));
    assert!(migration.up.contains("note TEXT"));
    assert!(!migration.up.contains("note TEXT NOT NULL"));
    assert!(migration.up.contains("PRIMARY KEY (id)"));
    assert!(migration.down.contains("DROP TABLE accounts;"));
}

#[test]
fn blocking_migration_carries_warning_comment() {
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
  status String
}
"#,
    ));
    let migration = emit(&diff(&prev, &next));
    assert!(migration.has_blocking);
    assert!(migration.up.contains("WARNING"));
    assert!(
        migration
            .up
            .contains("ALTER TABLE accounts ADD COLUMN status TEXT NOT NULL")
    );
}

#[test]
fn adding_dbgenerated_required_column_is_blocking() {
    // A `dbgenerated()` default backfills nothing the diff engine can
    // prove — adding it as a Required, non-PK column to an existing
    // table must be classified the same as "no default at all".
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
  status String @default(dbgenerated())
}
"#,
    ));
    let migration = emit(&diff(&prev, &next));
    assert!(migration.has_blocking);
    assert!(migration.up.contains("WARNING"));
    assert!(
        migration
            .up
            .contains("ALTER TABLE accounts ADD COLUMN status TEXT NOT NULL;"),
        "up was: {}",
        migration.up
    );
    assert!(!migration.up.contains("DEFAULT dbgenerated()"));
}

#[test]
fn unique_field_creates_unique_index() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model User {
  id Int @id
  email String @unique
}
"#,
    ));
    let migration = emit(&diff(&prev, &next));
    assert!(
        migration
            .up
            .contains("CREATE UNIQUE INDEX users_email_key ON users (email);"),
        "up was: {}",
        migration.up
    );
    assert!(migration.down.contains("DROP INDEX users_email_key;"));
}

#[test]
fn reserved_column_name_is_quoted() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Item {
  id Int @id
  order Int
}
"#,
    ));
    let migration = emit(&diff(&prev, &next));
    assert!(migration.up.contains("\"order\" BIGINT NOT NULL"));
}

#[test]
fn defaults_are_rendered() {
    let prev = schema(&with_models(""));
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
            .contains("status TEXT NOT NULL DEFAULT 'pending'"),
        "up was: {}",
        migration.up
    );
}

#[test]
fn dbgenerated_default_emits_no_default_clause() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Article {
  id String @id @default(dbgenerated())
  createdAt DateTime @default(dbgenerated())
}
"#,
    ));
    let migration = emit(&diff(&prev, &next));
    assert!(
        !migration.up.contains("DEFAULT dbgenerated()"),
        "emitted DDL must never contain the literal invalid `DEFAULT dbgenerated()` call: {}",
        migration.up
    );
    assert!(
        migration.up.contains("id TEXT NOT NULL,"),
        "id column should carry NOT NULL but no DEFAULT clause: {}",
        migration.up
    );
    assert!(
        migration.up.contains("created_at TIMESTAMPTZ NOT NULL,"),
        "created_at column should carry NOT NULL but no DEFAULT clause: {}",
        migration.up
    );
}

#[test]
fn dbgenerated_required_column_is_flagged_as_unverified() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Article {
  id String @id @default(dbgenerated())
  createdAt DateTime @default(dbgenerated())
  title String
}
"#,
    ));
    let migration = emit(&diff(&prev, &next));
    assert_eq!(
        migration.unverified_dbgenerated,
        vec![
            ("articles".to_owned(), "id".to_owned()),
            ("articles".to_owned(), "created_at".to_owned()),
        ]
    );
    assert!(migration.up.contains("articles.id"), "up: {}", migration.up);
    assert!(
        migration.up.contains("articles.created_at"),
        "up: {}",
        migration.up
    );
}

#[test]
fn optional_dbgenerated_column_is_not_flagged() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Article {
  id Int @id
  publishedAt DateTime? @default(dbgenerated())
}
"#,
    ));
    let migration = emit(&diff(&prev, &next));
    assert!(migration.unverified_dbgenerated.is_empty());
}

#[test]
fn list_column_renders_as_array() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Tag {
  id Int @id
  names String[]
}
"#,
    ));
    let migration = emit(&diff(&prev, &next));
    assert!(
        migration.up.contains("names TEXT[] NOT NULL"),
        "up was: {}",
        migration.up
    );
}

#[test]
fn composite_primary_key_emits_multi_column_constraint() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Account {
  id Int @id
}

model AccountMembership {
  accountId Int
  subject String
  active Boolean

  @@id([accountId, subject])
}
"#,
    ));
    let migration = emit(&diff(&prev, &next));
    assert!(
        migration.up.contains("CREATE TABLE account_memberships"),
        "up was: {}",
        migration.up
    );
    assert!(
        migration.up.contains("PRIMARY KEY (account_id, subject)"),
        "up was: {}",
        migration.up
    );
    assert!(!migration.up.contains("PRIMARY KEY (account_id)"));
    assert!(!migration.up.contains("PRIMARY KEY (subject)"));
}

#[test]
fn empty_diff_produces_empty_migration() {
    let s = schema(&with_models(
        r#"
model Account {
  id Int @id
}
"#,
    ));
    let migration = emit(&diff(&s, &s));
    assert_eq!(migration.up.trim(), "");
    assert_eq!(migration.down.trim(), "");
    assert!(!migration.has_lossy);
    assert!(!migration.has_blocking);
}
