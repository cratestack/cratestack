//! Upsert rendering tests.

#![cfg(test)]

use cratestack_sql::{ConflictTarget, SqlColumnValue, SqlValue, SqliteDialect};

use super::tests_fixtures::fixture_descriptor;
use super::upsert::{render_upsert, render_upsert_with_conflict};

#[test]
fn upsert_with_composite_conflict_emits_tuple_in_on_conflict() {
    let dialect = SqliteDialect;
    let descriptor = fixture_descriptor();
    let (sql, _) = render_upsert_with_conflict(
        &dialect,
        &descriptor,
        &[
            SqlColumnValue { column: "title", value: SqlValue::String("hi".into()) },
            SqlColumnValue { column: "published", value: SqlValue::Bool(true) },
        ],
        ConflictTarget::Columns(&["title", "published"]),
    );
    assert!(
        sql.contains("ON CONFLICT (title, published) DO UPDATE SET"),
        "got: {sql}",
    );
}

#[test]
fn upsert_default_conflict_target_is_primary_key() {
    let dialect = SqliteDialect;
    let descriptor = fixture_descriptor();
    let (pk_sql, _) = render_upsert(
        &dialect,
        &descriptor,
        &[SqlColumnValue { column: "title", value: SqlValue::String("x".into()) }],
    );
    let (explicit_sql, _) = render_upsert_with_conflict(
        &dialect,
        &descriptor,
        &[SqlColumnValue { column: "title", value: SqlValue::String("x".into()) }],
        ConflictTarget::PrimaryKey,
    );
    assert_eq!(pk_sql, explicit_sql);
    assert!(pk_sql.contains("ON CONFLICT (id) DO UPDATE SET"));
}
