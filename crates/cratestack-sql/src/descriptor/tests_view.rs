//! Tests for [`super::ViewDescriptor`] — smoke-tests that the
//! trait impl resolves and that the default `select_projection` /
//! `select_projection_subset` impls behave correctly for views.

use super::{ModelColumn, ReadSource, ViewDescriptor};

struct DummyRow;
struct DummyPk;

const COLUMNS: &[ModelColumn] = &[
    ModelColumn {
        rust_name: "id",
        sql_name: "id",
    },
    ModelColumn {
        rust_name: "email",
        sql_name: "email",
    },
];

const SOURCES: &[&str] = &["Customer", "Order"];

fn descriptor() -> ViewDescriptor<DummyRow, DummyPk> {
    ViewDescriptor::new(
        "public",
        "active_customer",
        COLUMNS,
        "id",
        &["id", "email"],
        &["id", "email"],
        &[],
        &[],
        &[],
        &[],
        false,
        SOURCES,
    )
}

/// Compile-time proof that `ViewDescriptor` satisfies the
/// `ReadSource` bound the upcoming view-aware read builders will
/// take. Unused at runtime — its bound is the test.
#[allow(dead_code)]
fn accepts_read_source<S: ReadSource<DummyRow, DummyPk>>(_: &S) {}

#[test]
fn implements_read_source() {
    let d = descriptor();
    accepts_read_source(&d);
    assert_eq!(d.table_name(), "active_customer");
    assert_eq!(d.primary_key(), "id");
    assert_eq!(d.allowed_includes(), &[] as &[&str]);
    assert!(d.soft_delete_column().is_none());
    // The default `select_projection` impl from the trait should
    // produce the same shape as `ModelDescriptor` would.
    assert_eq!(
        d.select_projection(),
        "id AS \"id\", email AS \"email\"".to_owned()
    );
}

#[test]
fn select_projection_subset_falls_back_to_primary_key() {
    let d = descriptor();
    // When the requested columns don't match anything, the default
    // impl should still emit the primary key so the SQL is valid.
    assert_eq!(
        d.select_projection_subset(&["nope"]),
        "id AS \"id\"".to_owned()
    );
}
