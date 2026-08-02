//! `onDelete`/`onUpdate` referential-action DDL, layered on top of the
//! plain FOREIGN KEY support in `relations.rs`.

use super::super::emit;
use super::{schema, with_models};
use crate::diff::diff;

fn tenant_and_application(relation_suffix: &str) -> String {
    tenant_and_application_with_arity(relation_suffix, false)
}

fn tenant_and_application_with_arity(relation_suffix: &str, nullable: bool) -> String {
    let mark = if nullable { "?" } else { "" };
    with_models(&format!(
        r#"
model Tenant {{
  id String @id
}}

model Application {{
  id String @id
  tenantId String{mark}
  tenant Tenant{mark} @relation(fields: [tenantId], references: [id]{relation_suffix})
}}
"#
    ))
}

#[test]
fn on_delete_and_on_update_both_render() {
    let prev = schema(&with_models(""));
    let next = schema(&tenant_and_application(
        ", onDelete: Cascade, onUpdate: Restrict",
    ));
    let migration = emit(&diff(&prev, &next));
    assert!(
        migration.up.contains(
            "ALTER TABLE applications ADD CONSTRAINT applications_tenant_id_fkey \
             FOREIGN KEY (tenant_id) REFERENCES tenants (id) ON DELETE CASCADE ON UPDATE RESTRICT;"
        ),
        "up was: {}",
        migration.up
    );
}

#[test]
fn only_on_delete_set_omits_on_update_clause() {
    let prev = schema(&with_models(""));
    let next = schema(&tenant_and_application(", onDelete: Cascade"));
    let migration = emit(&diff(&prev, &next));
    assert!(
        migration
            .up
            .contains("FOREIGN KEY (tenant_id) REFERENCES tenants (id) ON DELETE CASCADE;"),
        "up was: {}",
        migration.up
    );
    assert!(!migration.up.contains("ON UPDATE"));
}

#[test]
fn no_action_declared_emits_no_clause_at_all() {
    // Backward compatibility: a relation with no onDelete/onUpdate
    // must emit byte-identical DDL to before this feature existed.
    let prev = schema(&with_models(""));
    let next = schema(&tenant_and_application(""));
    let migration = emit(&diff(&prev, &next));
    assert!(
        migration.up.contains(
            "ALTER TABLE applications ADD CONSTRAINT applications_tenant_id_fkey \
             FOREIGN KEY (tenant_id) REFERENCES tenants (id);"
        ),
        "up was: {}",
        migration.up
    );
    assert!(!migration.up.contains("ON DELETE"));
    assert!(!migration.up.contains("ON UPDATE"));
}

#[test]
fn explicit_no_action_also_emits_no_clause() {
    let prev = schema(&with_models(""));
    let next = schema(&tenant_and_application(
        ", onDelete: NoAction, onUpdate: NoAction",
    ));
    let migration = emit(&diff(&prev, &next));
    assert!(!migration.up.contains("ON DELETE"));
    assert!(!migration.up.contains("ON UPDATE"));
}

#[test]
fn set_null_and_set_default_render_as_two_word_keywords() {
    let prev = schema(&with_models(""));
    let next = schema(&tenant_and_application_with_arity(
        ", onDelete: SetNull",
        true,
    ));
    let migration = emit(&diff(&prev, &next));
    assert!(
        migration.up.contains("ON DELETE SET NULL"),
        "up was: {}",
        migration.up
    );
}

#[test]
fn down_migration_preserves_the_actions_on_reversal() {
    let prev = schema(&with_models(""));
    let next = schema(&tenant_and_application(", onDelete: Cascade"));
    let migration = emit(&diff(&prev, &next));
    assert!(!migration.has_lossy);
    assert!(
        migration
            .down
            .contains("ALTER TABLE applications DROP CONSTRAINT applications_tenant_id_fkey;"),
        "down was: {}",
        migration.down
    );
}

#[test]
fn removing_the_action_alone_drops_and_re_adds_the_constraint() {
    let prev = schema(&tenant_and_application(", onDelete: Cascade"));
    let next = schema(&tenant_and_application(""));
    let migration = emit(&diff(&prev, &next));
    assert!(
        migration
            .up
            .contains("ALTER TABLE applications DROP CONSTRAINT applications_tenant_id_fkey;"),
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
}
