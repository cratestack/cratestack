//! Op-level coverage for issue #260's foreign-key IR, independent of
//! any particular dialect's DDL rendering.

use super::super::diff;
use super::{schema, with_models};
use crate::ir::Op;

fn tenant_and_application() -> (String, String) {
    let prev = with_models("");
    let next = with_models(
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
    );
    (prev, next)
}

#[test]
fn new_relation_produces_an_add_foreign_key_op() {
    let (prev, next) = tenant_and_application();
    let ops = diff(&schema(&prev), &schema(&next));
    let fk = ops
        .iter()
        .find_map(|op| match op {
            Op::AddForeignKey(fk) => Some(fk),
            _ => None,
        })
        .expect("expected an AddForeignKey op");
    assert_eq!(fk.name, "applications_tenant_id_fkey");
    assert_eq!(fk.table, "applications");
    assert_eq!(fk.column, "tenant_id");
    assert_eq!(fk.referenced_table, "tenants");
    assert_eq!(fk.referenced_column, "id");
}

#[test]
fn back_reference_side_produces_no_op() {
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
    let ops = diff(&prev, &next);
    let fk_ops: Vec<_> = ops
        .iter()
        .filter(|op| matches!(op, Op::AddForeignKey(_)))
        .collect();
    assert_eq!(fk_ops.len(), 1, "expected exactly one AddForeignKey op");
}

#[test]
fn changing_a_relations_target_drops_and_re_adds_the_constraint() {
    let prev = schema(&with_models(
        r#"
model Tenant {
  id String @id
}

model Billing {
  id String @id
}

model Application {
  id String @id
  tenantId String
  tenant Tenant @relation(fields: [tenantId], references: [id])
}
"#,
    ));
    // Repoint the same local column at a different target model —
    // same fk_name (table + column unchanged), different definition.
    let next = schema(&with_models(
        r#"
model Tenant {
  id String @id
}

model Billing {
  id String @id
}

model Application {
  id String @id
  tenantId String
  tenant Billing @relation(fields: [tenantId], references: [id])
}
"#,
    ));
    let ops = diff(&prev, &next);
    let dropped = ops
        .iter()
        .find_map(|op| match op {
            Op::DropForeignKey(fk) => Some(fk),
            _ => None,
        })
        .expect("expected a DropForeignKey op for the old target");
    assert_eq!(dropped.referenced_table, "tenants");
    let added = ops
        .iter()
        .find_map(|op| match op {
            Op::AddForeignKey(fk) => Some(fk),
            _ => None,
        })
        .expect("expected an AddForeignKey op for the new target");
    assert_eq!(added.referenced_table, "billings");
}

#[test]
fn no_change_produces_no_foreign_key_ops() {
    let (_, next) = tenant_and_application();
    let s = schema(&next);
    let ops = diff(&s, &s);
    assert!(ops.is_empty());
}
