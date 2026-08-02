//! Regression coverage for issue #260: a declared `@relation` must
//! produce a real `FOREIGN KEY` constraint, not just a same-named
//! column with no referential integrity.

use super::super::emit;
use super::{schema, with_models};
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

#[test]
fn adding_a_relation_to_an_existing_table_emits_add_constraint() {
    let prev = schema(&with_models(
        r#"
model Tenant {
  id String @id
}

model Application {
  id String @id
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
  tenant Tenant @relation(fields: [tenantId], references: [id])
}
"#,
    ));
    let migration = emit(&diff(&prev, &next));
    assert!(
        migration
            .up
            .contains("ALTER TABLE applications ADD COLUMN tenant_id TEXT NOT NULL"),
        "up was: {}",
        migration.up
    );
    assert!(
        migration.up.contains(
            "ALTER TABLE applications ADD CONSTRAINT applications_tenant_id_fkey \
             FOREIGN KEY (tenant_id) REFERENCES tenants (id);"
        ),
        "up was: {}",
        migration.up
    );
    let add_column = migration.up.find("ADD COLUMN tenant_id").unwrap();
    let add_constraint = migration
        .up
        .find("ADD CONSTRAINT applications_tenant_id_fkey")
        .unwrap();
    assert!(add_column < add_constraint);
}

#[test]
fn removing_a_relation_emits_drop_constraint() {
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
            .contains("ALTER TABLE applications DROP CONSTRAINT applications_tenant_id_fkey;"),
        "up was: {}",
        migration.up
    );
    assert!(
        migration.down.contains(
            "ALTER TABLE applications ADD CONSTRAINT applications_tenant_id_fkey \
             FOREIGN KEY (tenant_id) REFERENCES tenants (id);"
        ),
        "down was: {}",
        migration.down
    );
}

#[test]
fn dropping_both_related_tables_drops_child_before_parent() {
    // Postgres refuses `DROP TABLE tenants` while `applications`'s FK
    // still references it. Alphabetically "applications" < "tenants"
    // already, so also cover a name pair where the naive alphabetical
    // order would get it backwards.
    let prev = schema(&with_models(
        r#"
model Application {
  id String @id
}

model Zeta {
  id String @id
  applicationId String
  application Application @relation(fields: [applicationId], references: [id])
}
"#,
    ));
    let next = schema(&with_models(""));
    let migration = emit(&diff(&prev, &next));
    assert!(migration.has_lossy);
    let drop_zeta = migration.up.find("DROP TABLE zetas").unwrap();
    let drop_application = migration.up.find("DROP TABLE applications").unwrap();
    assert!(
        drop_zeta < drop_application,
        "child table `zetas` (holds the FK) must drop before parent `applications`: {}",
        migration.up
    );
}

#[test]
fn foreign_key_ops_are_safe_not_blocking() {
    use crate::ir::Destructiveness;

    let prev = schema(&with_models(
        r#"
model Tenant {
  id String @id
}

model Application {
  id String @id
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
  tenant Tenant @relation(fields: [tenantId], references: [id])
}
"#,
    ));
    let ops = diff(&prev, &next);
    let fk_op = ops
        .iter()
        .find(|op| matches!(op, crate::ir::Op::AddForeignKey(_)))
        .expect("expected an AddForeignKey op");
    assert_eq!(fk_op.destructiveness(), Destructiveness::Safe);
}
