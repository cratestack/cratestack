//! Shared fixtures used by every render-test submodule.

#![cfg(test)]

use cratestack_sql::{ModelColumn, ModelDescriptor};

pub(super) fn fixture_descriptor() -> ModelDescriptor<(), i64> {
    const COLUMNS: &[ModelColumn] = &[
        ModelColumn { rust_name: "id", sql_name: "id" },
        ModelColumn { rust_name: "title", sql_name: "title" },
        ModelColumn { rust_name: "published", sql_name: "published" },
    ];
    ModelDescriptor::new(
        "Post",
        "posts",
        COLUMNS,
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
}

pub(super) fn soft_delete_descriptor() -> ModelDescriptor<(), i64> {
    let mut d = fixture_descriptor();
    d.soft_delete_column = Some("deleted_at");
    d
}
