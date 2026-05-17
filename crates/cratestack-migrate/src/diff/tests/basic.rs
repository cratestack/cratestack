use super::super::diff;
use super::{schema, with_models};
use crate::ir::{Destructiveness, Op};

#[test]
fn empty_to_empty_produces_no_ops() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(""));
    assert!(diff(&prev, &next).is_empty());
}

#[test]
fn adding_a_model_emits_create_table() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Account {
  id Int @id
  balance Int
}
"#,
    ));
    let ops = diff(&prev, &next);
    assert_eq!(ops.len(), 1);
    match &ops[0] {
        Op::CreateTable(create) => {
            assert_eq!(create.name, "accounts");
            assert_eq!(create.columns.len(), 2);
            assert_eq!(create.columns[0].name, "id");
            assert!(create.columns[0].primary_key);
            assert_eq!(create.columns[1].name, "balance");
            assert!(!create.columns[1].primary_key);
        }
        other => panic!("expected CreateTable, got {other:?}"),
    }
}

#[test]
fn removing_a_model_emits_drop_table() {
    let prev = schema(&with_models(
        r#"
model Account {
  id Int @id
}
"#,
    ));
    let next = schema(&with_models(""));
    let ops = diff(&prev, &next);
    assert_eq!(ops.len(), 1);
    assert!(matches!(&ops[0], Op::DropTable(drop) if drop.name == "accounts"));
    assert!(matches!(ops[0].destructiveness(), Destructiveness::Lossy));
}

#[test]
fn adding_a_column_emits_add_column() {
    let prev = schema(&with_models(
        r#"
model Account {
  id Int @id
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model Account {
  id Int @id
  balance Int?
}
"#,
    ));
    let ops = diff(&prev, &next);
    assert_eq!(ops.len(), 1);
    match &ops[0] {
        Op::AddColumn(add) => {
            assert_eq!(add.table, "accounts");
            assert_eq!(add.column.name, "balance");
            assert!(matches!(add.column.arity, crate::ir::ColumnArity::Optional));
        }
        other => panic!("expected AddColumn, got {other:?}"),
    }
    assert!(matches!(ops[0].destructiveness(), Destructiveness::Safe));
}

#[test]
fn adding_required_column_without_default_is_blocking() {
    let prev = schema(&with_models(
        r#"
model Account {
  id Int @id
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model Account {
  id Int @id
  status String
}
"#,
    ));
    let ops = diff(&prev, &next);
    assert_eq!(ops.len(), 1);
    assert!(matches!(
        ops[0].destructiveness(),
        Destructiveness::Blocking
    ));
}

#[test]
fn adding_required_column_with_default_is_safe() {
    let prev = schema(&with_models(
        r#"
model Account {
  id Int @id
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model Account {
  id Int @id
  status String @default('pending')
}
"#,
    ));
    let ops = diff(&prev, &next);
    assert_eq!(ops.len(), 1);
    assert!(matches!(ops[0].destructiveness(), Destructiveness::Safe));
}

#[test]
fn removing_a_column_emits_drop_column() {
    let prev = schema(&with_models(
        r#"
model Account {
  id Int @id
  legacy String?
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model Account {
  id Int @id
}
"#,
    ));
    let ops = diff(&prev, &next);
    assert_eq!(ops.len(), 1);
    assert!(matches!(&ops[0], Op::DropColumn(drop)
        if drop.table == "accounts" && drop.column == "legacy"));
    assert!(matches!(ops[0].destructiveness(), Destructiveness::Lossy));
}
