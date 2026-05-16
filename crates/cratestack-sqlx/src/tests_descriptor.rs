#![cfg(test)]

use crate::{ModelColumn, ModelDescriptor, SqlColumnValue, SqlValue};

#[test]
fn select_projection_aliases_sql_columns_to_rust_fields() {
    let descriptor = ModelDescriptor::<(), i64>::new(
        "Post",
        "posts",
        &[
            ModelColumn {
                rust_name: "id",
                sql_name: "id",
            },
            ModelColumn {
                rust_name: "authorId",
                sql_name: "author_id",
            },
        ],
        "id",
        &["id", "authorId"],
        &["author"],
        &["id", "authorId", "author.email"],
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
    );

    assert_eq!(
        descriptor.select_projection(),
        "id AS \"id\", author_id AS \"authorId\""
    );
}

#[test]
fn create_preview_sql_numbers_placeholders() {
    let values = [
        SqlColumnValue {
            column: "title",
            value: SqlValue::String("hello".to_owned()),
        },
        SqlColumnValue {
            column: "published",
            value: SqlValue::Bool(true),
        },
    ];

    let columns = values
        .iter()
        .map(|value| value.column)
        .collect::<Vec<_>>()
        .join(", ");
    let placeholders = (1..=values.len())
        .map(|index| format!("${index}"))
        .collect::<Vec<_>>()
        .join(", ");

    assert_eq!(columns, "title, published");
    assert_eq!(placeholders, "$1, $2");
}
