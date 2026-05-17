use super::super::diff;
use super::{schema, with_models};
use crate::ir::{Destructiveness, Op};

#[test]
fn unique_field_emits_unique_index_on_create() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model User {
  id Int @id
  email String @unique
}
"#,
    ));
    let ops = diff(&prev, &next);
    // CreateTable + AddIndex
    assert_eq!(ops.len(), 2);
    assert!(matches!(&ops[0], Op::CreateTable(_)));
    match &ops[1] {
        Op::AddIndex(index) => {
            assert_eq!(index.name, "users_email_key");
            assert_eq!(index.table, "users");
            assert_eq!(index.columns, vec!["email".to_owned()]);
            assert!(index.unique);
        }
        other => panic!("expected AddIndex, got {other:?}"),
    }
}

#[test]
fn dropping_unique_emits_drop_index() {
    let prev = schema(&with_models(
        r#"
model User {
  id Int @id
  email String @unique
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model User {
  id Int @id
  email String
}
"#,
    ));
    let ops = diff(&prev, &next);
    assert_eq!(ops.len(), 1);
    assert!(matches!(&ops[0], Op::DropIndex(drop)
        if drop.name == "users_email_key" && drop.table == "users"));
}

#[test]
fn ops_are_ordered_drops_before_creates_indexes_last() {
    let prev = schema(&with_models(
        r#"
model Old {
  id Int @id
}
"#,
    ));
    let next = schema(&with_models(
        r#"
model New {
  id Int @id
  email String @unique
}
"#,
    ));
    let ops = diff(&prev, &next);
    // Order: DropTable(old), CreateTable(new), AddIndex(new.email)
    assert_eq!(ops.len(), 3);
    assert!(matches!(&ops[0], Op::DropTable(_)));
    assert!(matches!(&ops[1], Op::CreateTable(_)));
    assert!(matches!(&ops[2], Op::AddIndex(_)));
}

#[test]
fn primary_key_required_column_is_safe_to_add() {
    // Adding an `@id` column with no default would otherwise be
    // blocking, but tables created in the same migration imply the
    // column lands at table-create time, not as an alter. This test
    // exercises the *column-level* destructiveness call directly to
    // confirm PK acts as a backfill source.
    use crate::ir::{Column, ColumnArity, ColumnType};
    let column = Column {
        name: "id".to_owned(),
        ty: ColumnType::Scalar("Int".to_owned()),
        arity: ColumnArity::Required,
        default: None,
        primary_key: true,
    };
    assert!(matches!(
        column.destructiveness_on_add(),
        Destructiveness::Safe
    ));
}
