//! Coverage for a `@relation` added to, removed from, or interacting
//! with tables that already exist across the diff.

use super::super::super::emit;
use super::super::{schema, with_models};
use crate::diff::diff;

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
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
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
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
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
    let migration = emit(&diff(&prev, &next).expect("diff should succeed"));
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
    let ops = diff(&prev, &next).expect("diff should succeed");
    let fk_op = ops
        .iter()
        .find(|op| matches!(op, crate::ir::Op::AddForeignKey(_)))
        .expect("expected an AddForeignKey op");
    assert_eq!(fk_op.destructiveness(), Destructiveness::Safe);
}
