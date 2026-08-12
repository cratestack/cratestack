//! SQLite DDL for model-level `@@unique([...])` (issue #262).
//!
//! The embedded backend gets the same constraint as Postgres so a
//! schema means the same thing on both — including for SQLite's own
//! `ON CONFLICT` upsert form, which needs a matching unique index.

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
    assert!(
        migration
            .down
            .contains("DROP INDEX applications_tenant_id_name_environment_key;"),
        "down was: {}",
        migration.down
    );
}

#[test]
fn several_composite_uniques_emit_one_index_each() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Application {
  id String @id
  tenantId String
  name String
  slug String

  @@unique([tenantId, name])
  @@unique([tenantId, slug])
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration.up.contains(
            "CREATE UNIQUE INDEX applications_tenant_id_name_key ON applications (tenant_id, name);"
        ),
        "up was: {}",
        migration.up
    );
    assert!(
        migration.up.contains(
            "CREATE UNIQUE INDEX applications_tenant_id_slug_key ON applications (tenant_id, slug);"
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
