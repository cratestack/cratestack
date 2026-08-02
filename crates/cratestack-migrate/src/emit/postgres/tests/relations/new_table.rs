//! Coverage for a `@relation` declared alongside brand-new tables
//! (both sides of the relation appear in the same migration).

use super::super::super::emit;
use super::super::{schema, with_models};
use crate::diff::diff;

#[test]
fn relation_emits_foreign_key_constraint() {
    // The issue's own repro: a `tenantId` column linked via
    // `@relation` must gain a real `FOREIGN KEY ... REFERENCES`, not
    // silently emit no constraint at all.
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Tenant {
  id String @id
  name String
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
    assert!(!migration.has_blocking);
    assert!(
        migration.up.contains(
            "ALTER TABLE applications ADD CONSTRAINT applications_tenant_id_fkey \
             FOREIGN KEY (tenant_id) REFERENCES tenants (id);"
        ),
        "up was: {}",
        migration.up
    );
    // The back-reference (`Post[]`-shaped) side carries its own
    // `@relation` too, but it has no physical column and must not
    // produce a second, backwards constraint.
    assert_eq!(
        migration.up.matches("FOREIGN KEY").count(),
        1,
        "up was: {}",
        migration.up
    );
    assert!(
        migration
            .down
            .contains("ALTER TABLE applications DROP CONSTRAINT applications_tenant_id_fkey;"),
        "down was: {}",
        migration.down
    );
}

#[test]
fn list_side_relation_field_produces_no_constraint_of_its_own() {
    // The inverse ("has many") side declares `@relation` with fields
    // swapped (`fields: [id], references: [tenantId]`) purely to
    // satisfy the parser's two-sided requirement. Emitting a
    // constraint for it would be backwards SQL (`tenants.id
    // REFERENCES applications.tenant_id`).
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Tenant {
  id String @id
  applications Application[] @relation(fields: [id], references: [tenantId])
}

model Application {
  id String @id
  tenantId String
  tenant Tenant @relation(fields: [tenantId], references: [id])
}
"#,
    ));
    let migration = emit(&diff(&prev, &next));
    assert_eq!(
        migration.up.matches("FOREIGN KEY").count(),
        1,
        "up was: {}",
        migration.up
    );
    assert!(!migration.up.contains("ALTER TABLE tenants ADD CONSTRAINT"));
}

#[test]
fn foreign_key_lands_after_both_create_tables() {
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
    let create_tenants = migration.up.find("CREATE TABLE tenants").unwrap();
    let create_applications = migration.up.find("CREATE TABLE applications").unwrap();
    let add_constraint = migration
        .up
        .find("ADD CONSTRAINT applications_tenant_id_fkey")
        .expect("expected FK constraint in up.sql");
    assert!(add_constraint > create_tenants);
    assert!(add_constraint > create_applications);
}
