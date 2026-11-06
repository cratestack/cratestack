use crate::{ModelColumn, ModelDescriptor, SqlColumnValue, SqlValue};

use super::update_lock;

const fn column(name: &'static str) -> ModelColumn {
    ModelColumn {
        rust_name: name,
        sql_name: name,
    }
}

static COLUMNS: [ModelColumn; 3] = [column("id"), column("code"), column("n")];

fn set(column: &'static str) -> SqlColumnValue {
    SqlColumnValue {
        column,
        value: SqlValue::Int(1),
    }
}

#[test]
fn only_an_update_of_a_key_column_reads_under_for_update() {
    let descriptor = ModelDescriptor::<(), i64>::new(
        "Note",
        "notes",
        &COLUMNS,
        "id",
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        None,
        false,
        &[],
        &[],
        None,
        None,
        &[],
    )
    .with_unique_columns(&["code"]);
    assert_eq!(update_lock(&descriptor, &[set("n")]), " FOR NO KEY UPDATE");
    assert_eq!(
        update_lock(&descriptor, &[set("n"), set("code")]),
        " FOR UPDATE"
    );
    assert_eq!(update_lock(&descriptor, &[set("id")]), " FOR UPDATE");
}
