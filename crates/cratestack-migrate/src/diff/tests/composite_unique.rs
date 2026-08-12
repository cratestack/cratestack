//! Diff coverage for model-level `@@unique([...])` composite unique
//! constraints (issue #262). Single-field `@unique` lives in
//! [`super::indexes`]; the SQL these ops turn into is asserted in the
//! per-backend emitter tests.

use super::super::diff;
use super::{schema, with_models};
use crate::ir::Op;

#[test]
fn composite_unique_emits_multi_column_index_on_create() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Application {
  id String @id
  tenantId String
  name String
  environment String

  @@unique([tenantId, name, environment])
}
"#,
    ));
    let ops = diff(&prev, &next).expect("diff should succeed");
    // CreateTable + AddIndex
    assert_eq!(ops.len(), 2, "ops: {ops:?}");
    match &ops[1] {
        Op::AddIndex(index) => {
            assert_eq!(index.name, "applications_tenant_id_name_environment_key");
            assert_eq!(index.table, "applications");
            assert_eq!(
                index.columns,
                vec![
                    "tenant_id".to_owned(),
                    "name".to_owned(),
                    "environment".to_owned(),
                ]
            );
            assert!(index.unique);
        }
        other => panic!("expected AddIndex, got {other:?}"),
    }
}

#[test]
fn composite_unique_coexists_with_field_level_unique() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Application {
  id String @id
  slug String @unique
  tenantId String
  name String

  @@unique([tenantId, name])
}
"#,
    ));
    let ops = diff(&prev, &next).expect("diff should succeed");
    let names: Vec<&str> = ops
        .iter()
        .filter_map(|op| match op {
            Op::AddIndex(index) => Some(index.name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        names,
        vec!["applications_slug_key", "applications_tenant_id_name_key"],
    );
}

#[test]
fn several_composite_uniques_each_get_their_own_index() {
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
    let ops = diff(&prev, &next).expect("diff should succeed");
    let names: Vec<&str> = ops
        .iter()
        .filter_map(|op| match op {
            Op::AddIndex(index) => Some(index.name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        names,
        vec![
            "applications_tenant_id_name_key",
            "applications_tenant_id_slug_key",
        ],
    );
}

#[test]
fn dropping_composite_unique_emits_drop_index() {
    let prev = schema(&with_models(
        r#"
model Application {
  id String @id
  tenantId String
  name String

  @@unique([tenantId, name])
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model Application {
  id String @id
  tenantId String
  name String
}
"#,
    ));
    let ops = diff(&prev, &next).expect("diff should succeed");
    assert_eq!(ops.len(), 1, "ops: {ops:?}");
    assert!(matches!(&ops[0], Op::DropIndex(drop)
        if drop.name == "applications_tenant_id_name_key" && drop.table == "applications"));
}

#[test]
fn reordering_composite_unique_fields_replaces_the_index() {
    // Column order is part of the constraint's identity (it decides
    // which prefix lookups the index serves), so a reorder is a real
    // schema change, not a no-op.
    let prev = schema(&with_models(
        r#"
model Application {
  id String @id
  tenantId String
  name String

  @@unique([tenantId, name])
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model Application {
  id String @id
  tenantId String
  name String

  @@unique([name, tenantId])
}
"#,
    ));
    let ops = diff(&prev, &next).expect("diff should succeed");
    assert_eq!(ops.len(), 2, "ops: {ops:?}");
    assert!(matches!(&ops[0], Op::DropIndex(drop)
        if drop.name == "applications_tenant_id_name_key"));
    assert!(matches!(&ops[1], Op::AddIndex(index)
        if index.name == "applications_name_tenant_id_key"
            && index.columns == vec!["name".to_owned(), "tenant_id".to_owned()]));
}

#[test]
fn unchanged_composite_unique_produces_no_ops() {
    let source = with_models(
        r#"
model Application {
  id String @id
  tenantId String
  name String

  @@unique([tenantId, name])
}
"#,
    );
    let s = schema(&source);
    assert!(diff(&s, &s).expect("diff should succeed").is_empty());
}
