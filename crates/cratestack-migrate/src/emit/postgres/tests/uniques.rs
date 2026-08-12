//! Postgres DDL for model-level `@@unique([...])` (issue #262).
//!
//! The point of the constraint is not only integrity: Postgres will
//! only accept `ON CONFLICT (a, b, c) DO UPDATE` when a unique index
//! over exactly that tuple exists, so an upsert-based idempotency
//! design depends on this DDL actually being emitted.

use super::super::emit;
use super::{schema, with_models};
use crate::diff::diff;

const APPLICATIONS: &str = r#"
model Application {
  id String @id
  tenantId String
  name String
  environment String

  @@unique([tenantId, name, environment])
}
"#;

#[test]
fn composite_unique_emits_create_unique_index() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(APPLICATIONS));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration.up.contains(
            "CREATE UNIQUE INDEX applications_tenant_id_name_environment_key \
             ON applications (tenant_id, name, environment);"
        ),
        "up was: {}",
        migration.up
    );
}

#[test]
fn composite_unique_index_is_reversed_in_down() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(APPLICATIONS));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(!migration.has_lossy, "up was: {}", migration.up);
    assert!(
        migration
            .down
            .contains("DROP INDEX applications_tenant_id_name_environment_key;"),
        "down was: {}",
        migration.down
    );
}

#[test]
fn adding_composite_unique_to_an_existing_table_only_emits_the_index() {
    let prev = schema(&with_models(
        r#"
model Application {
  id String @id
  tenantId String
  name String
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model Application {
  id String @id
  tenantId String
  name String

  @@unique([tenantId, name])
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        !migration.up.contains("CREATE TABLE"),
        "up: {}",
        migration.up
    );
    assert!(
        migration.up.contains(
            "CREATE UNIQUE INDEX applications_tenant_id_name_key ON applications (tenant_id, name);"
        ),
        "up was: {}",
        migration.up
    );
}

#[test]
fn dropping_composite_unique_emits_drop_index() {
    let prev = schema(&with_models(APPLICATIONS));
    let next = schema(&with_models(
        r#"
model Application {
  id String @id
  tenantId String
  name String
  environment String
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration
            .up
            .contains("DROP INDEX applications_tenant_id_name_environment_key;"),
        "up was: {}",
        migration.up
    );
}

#[test]
fn composite_unique_quotes_reserved_column_names() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Membership {
  id String @id
  user String
  group String

  @@unique([user, group])
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration.up.contains(
            "CREATE UNIQUE INDEX memberships_user_group_key \
             ON memberships (\"user\", \"group\");"
        ),
        "up was: {}",
        migration.up
    );
}
