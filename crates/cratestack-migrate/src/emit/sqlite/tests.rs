mod uniques;

use super::emit;
use crate::diff::diff;
use cratestack_core::Schema;
use cratestack_parser::parse_schema;

fn schema(source: &str) -> Schema {
    parse_schema(source).expect("schema should parse")
}

fn with_models(models: &str) -> String {
    format!(
        r#"
datasource db {{
  provider = "sqlite"
  url = env("DATABASE_URL")
}}
{models}
"#
    )
}

#[test]
fn create_table_emits_blob_columns() {
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
    assert!(migration.up.contains("CREATE TABLE accounts"));
    // Every scalar maps to BLOB per the rusqlite affinity contract.
    assert!(migration.up.contains("id BLOB NOT NULL"));
    assert!(migration.up.contains("balance BLOB NOT NULL"));
    assert!(migration.up.contains("note BLOB"));
    assert!(!migration.up.contains("note BLOB NOT NULL"));
    assert!(migration.up.contains("PRIMARY KEY (id)"));
    assert!(!migration.up.contains("BIGINT"));
    assert!(!migration.up.contains("TEXT"));
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
    assert!(migration.up.contains("CREATE TABLE account_memberships"));
    assert!(
        migration.up.contains("PRIMARY KEY (account_id, subject)"),
        "up was: {}",
        migration.up
    );
    assert!(!migration.up.contains("PRIMARY KEY (account_id)"));
    assert!(!migration.up.contains("PRIMARY KEY (subject)"));
}

#[test]
fn add_and_drop_column_use_alter_table() {
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
  balance Int?
}
"#,
    ));
    let migration = emit(&diff(&prev, &next));
    assert!(
        migration
            .up
            .contains("ALTER TABLE accounts DROP COLUMN legacy;")
    );
    assert!(
        migration
            .up
            .contains("ALTER TABLE accounts ADD COLUMN balance BLOB")
    );
}

#[test]
fn lossy_migration_uses_raise_fail_stub() {
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
    assert!(migration.down.contains("RAISE(FAIL"));
    assert!(migration.down.contains("DropColumn accounts.legacy"));
}

#[test]
fn unique_index_emits_create_unique_index() {
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
}

#[test]
fn defaults_pass_through_unchanged() {
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
            .contains("status BLOB NOT NULL DEFAULT 'pending'")
    );
}

#[test]
fn dbgenerated_default_emits_no_default_clause() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Article {
  id String @id @default(dbgenerated())
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
        migration.up.contains("id BLOB NOT NULL,"),
        "up was: {}",
        migration.up
    );
    assert_eq!(
        migration.unverified_dbgenerated,
        vec![("articles".to_owned(), "id".to_owned())]
    );
}

#[test]
fn enum_changes_produce_no_sqlite_ddl() {
    let prev = schema(&with_models(
        r#"
enum OrderStatus {
  Pending
}

model Order {
  id Int @id
  status OrderStatus
}
"#,
    ));
    let next = schema(&with_models(
        r#"
enum OrderStatus {
  Pending
  Shipped
}

model Order {
  id Int @id
  status OrderStatus
}
"#,
    ));
    let migration = emit(&diff(&prev, &next));
    // Variant added on the .cstack side; SQLite emits nothing
    // and the migration is not flagged destructive.
    assert!(!migration.has_lossy);
    assert_eq!(migration.up.trim(), "");
    assert_eq!(migration.down.trim(), "");
}

#[test]
fn enum_column_renders_as_blob_on_sqlite() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
enum OrderStatus {
  Pending
}

model Order {
  id Int @id
  status OrderStatus
}
"#,
    ));
    let migration = emit(&diff(&prev, &next));
    // No CREATE TYPE — SQLite has none.
    assert!(!migration.up.contains("CREATE TYPE"));
    // Column still lands as BLOB.
    assert!(migration.up.contains("status BLOB NOT NULL"));
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
}

// Regression coverage for issue #260. SQLite has no `ALTER TABLE ADD
// CONSTRAINT`, so unlike Postgres a declared `@relation` can't become
// a real, enforced constraint without a full table-rebuild dance —
// out of scope here (see `emit::sqlite::foreign_keys` docs). What
// SQLite *can* do, and previously didn't, is say so: a marker comment
// naming the constraint that would otherwise silently not exist.

#[test]
fn relation_emits_rebuild_marker_comment() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Tenant {
  id String @id
}

model Application {
  id String @id
  tenantId String
  tenant Tenant @relation(fields: [tenantId], references: [id])
}
"#,
    ));
    let migration = emit(&diff(&prev, &next));
    assert!(!migration.has_lossy);
    assert!(
        migration.up.contains(
            "-- SQLite: ADD CONSTRAINT applications_tenant_id_fkey FOREIGN KEY (tenant_id) \
             REFERENCES tenants (id) — requires table rebuild on SQLite."
        ),
        "up was: {}",
        migration.up
    );
    // No real constraint is emitted — SQLite genuinely can't add one
    // outside CREATE TABLE.
    assert!(
        !migration
            .up
            .contains("ALTER TABLE applications ADD CONSTRAINT")
    );
}

#[test]
fn removing_a_relation_emits_rebuild_marker_comment() {
    let prev = schema(&with_models(
        r#"
model Tenant {
  id String @id
}

model Application {
  id String @id
  tenantId String
  tenant Tenant @relation(fields: [tenantId], references: [id])
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model Tenant {
  id String @id
}

model Application {
  id String @id
  tenantId String
}
"#,
    ));
    let migration = emit(&diff(&prev, &next));
    assert!(!migration.has_lossy);
    assert!(
        migration
            .up
            .contains("-- SQLite: DROP CONSTRAINT applications_tenant_id_fkey"),
        "up was: {}",
        migration.up
    );
}

#[test]
fn relation_with_actions_mentions_them_in_the_rebuild_marker_comment() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Tenant {
  id String @id
}

model Application {
  id String @id
  tenantId String
  tenant Tenant @relation(fields: [tenantId], references: [id], onDelete: Cascade, onUpdate: Restrict)
}
"#,
    ));
    let migration = emit(&diff(&prev, &next));
    assert!(
        migration.up.contains(
            "-- SQLite: ADD CONSTRAINT applications_tenant_id_fkey FOREIGN KEY (tenant_id) \
             REFERENCES tenants (id) ON DELETE CASCADE ON UPDATE RESTRICT — requires table \
             rebuild on SQLite."
        ),
        "up was: {}",
        migration.up
    );
}

#[test]
fn relation_marker_comment_quotes_reserved_local_column_name() {
    // Review finding: the marker comment's identifiers weren't quoted,
    // so a local FK column that collides with a SQLite reserved word
    // (like `order`) rendered as a bare, ambiguous-looking token —
    // mirrors the Postgres emitter's `reserved_column_name_is_quoted`.
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Target {
  id Int @id
}

model Item {
  id Int @id
  order Int
  ref Target @relation(fields: [order], references: [id])
}
"#,
    ));
    let migration = emit(&diff(&prev, &next));
    assert!(
        migration
            .up
            .contains("FOREIGN KEY (\"order\") REFERENCES targets (id)"),
        "up was: {}",
        migration.up
    );
}
