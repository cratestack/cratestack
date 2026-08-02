//! Op-level coverage for `onDelete`/`onUpdate` on the foreign-key IR.

use super::super::diff;
use super::{schema, with_models};
use crate::ir::{ForeignKeyAction, Op};

#[test]
fn parsed_actions_populate_the_ir() {
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
    let ops = diff(&prev, &next);
    let fk = ops
        .iter()
        .find_map(|op| match op {
            Op::AddForeignKey(fk) => Some(fk),
            _ => None,
        })
        .expect("expected an AddForeignKey op");
    assert_eq!(fk.on_delete, ForeignKeyAction::Cascade);
    assert_eq!(fk.on_update, ForeignKeyAction::Restrict);
}

#[test]
fn omitted_actions_default_to_no_action() {
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
    let ops = diff(&prev, &next);
    let fk = ops
        .iter()
        .find_map(|op| match op {
            Op::AddForeignKey(fk) => Some(fk),
            _ => None,
        })
        .expect("expected an AddForeignKey op");
    assert_eq!(fk.on_delete, ForeignKeyAction::NoAction);
    assert_eq!(fk.on_update, ForeignKeyAction::NoAction);
}

#[test]
fn changing_only_the_action_drops_and_re_adds_the_constraint() {
    let prev = schema(&with_models(
        r#"
model Tenant {
  id String @id
}

model Application {
  id String @id
  tenantId String
  tenant Tenant @relation(fields: [tenantId], references: [id], onDelete: Cascade)
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
  tenant Tenant @relation(fields: [tenantId], references: [id], onDelete: Restrict)
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
        .expect("expected a DropForeignKey op for the old action");
    assert_eq!(dropped.on_delete, ForeignKeyAction::Cascade);
    let added = ops
        .iter()
        .find_map(|op| match op {
            Op::AddForeignKey(fk) => Some(fk),
            _ => None,
        })
        .expect("expected an AddForeignKey op for the new action");
    assert_eq!(added.on_delete, ForeignKeyAction::Restrict);
}
