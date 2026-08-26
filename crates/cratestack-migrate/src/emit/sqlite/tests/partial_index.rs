//! SQLite DDL for the `where: "<sql predicate>"` keyword argument on
//! `@@unique([...])` and `@@index([...])` (issue #742). SQLite has
//! supported the same `CREATE INDEX ... WHERE <predicate>` syntax as
//! Postgres since 3.8.0 (see `emit::sqlite::indexes`'s doc for the one
//! real divergence: what a predicate may legally *reference*, not the
//! syntax to render one).

use super::super::emit;
use super::{schema, with_models};
use crate::diff::diff;

#[test]
fn composite_unique_with_where_emits_partial_unique_index() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Payment {
  id String @id
  idempotencyKey String?
  amount Int

  @@unique([idempotencyKey], where: "idempotency_key IS NOT NULL")
}
"#,
    ));
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
    assert!(
        migration.up.contains(
            "CREATE UNIQUE INDEX payments_idempotency_key_key ON payments (idempotency_key) \
             WHERE idempotency_key IS NOT NULL;"
        ),
        "up was: {}",
        migration.up
    );
}

#[test]
fn no_where_predicate_renders_byte_identical_ddl() {
    let prev = schema(&with_models(""));
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
        migration.up.contains(
            "CREATE UNIQUE INDEX applications_tenant_id_name_key ON applications (tenant_id, name);"
        ),
        "up was: {}",
        migration.up
    );
    assert!(!migration.up.contains("WHERE"), "up was: {}", migration.up);
}
