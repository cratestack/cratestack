//! SELECT / ORDER BY rendering tests.

#![cfg(test)]

use cratestack_sql::{FieldRef, FilterExpr, OrderClause, SortDirection, SqlValue, SqliteDialect};

use super::select::{render_select, render_select_by_pk};
use super::tests_fixtures::{fixture_descriptor, soft_delete_descriptor};

#[test]
fn select_uses_question_placeholders_for_limit_and_offset() {
    let dialect = SqliteDialect;
    let descriptor = fixture_descriptor();
    let (sql, binds) = render_select(
        &dialect,
        &descriptor,
        &[],
        &[],
        Some(10),
        Some(5),
    );
    assert!(sql.contains("LIMIT ?1"));
    assert!(sql.contains("OFFSET ?2"));
    assert_eq!(binds, vec![SqlValue::Int(10), SqlValue::Int(5)]);
}

#[test]
fn select_with_filter_emits_where_and_binds_value() {
    let dialect = SqliteDialect;
    let descriptor = fixture_descriptor();
    let id_ref = FieldRef::<(), i64>::new("id");
    let (sql, binds) = render_select(
        &dialect,
        &descriptor,
        &[FilterExpr::from(id_ref.eq(42i64))],
        &[],
        None,
        None,
    );
    assert!(sql.contains("WHERE id = ?1"), "got: {sql}");
    assert_eq!(binds, vec![SqlValue::Int(42)]);
}

#[test]
fn select_with_soft_delete_filters_out_deleted_rows() {
    let dialect = SqliteDialect;
    let descriptor = soft_delete_descriptor();
    let (sql, _) = render_select(&dialect, &descriptor, &[], &[], None, None);
    assert!(sql.contains("WHERE deleted_at IS NULL"), "got: {sql}");
}

#[test]
fn select_by_pk_binds_id_and_filters_soft_deletes() {
    let dialect = SqliteDialect;
    let descriptor = soft_delete_descriptor();
    let (sql, binds) = render_select_by_pk(&dialect, &descriptor, SqlValue::Int(7));
    assert!(sql.contains("WHERE id = ?1"));
    assert!(sql.contains("AND deleted_at IS NULL"));
    assert_eq!(binds, vec![SqlValue::Int(7)]);
}

#[test]
fn order_by_appends_nulls_last() {
    let dialect = SqliteDialect;
    let descriptor = fixture_descriptor();
    let (sql, _) = render_select(
        &dialect,
        &descriptor,
        &[],
        &[OrderClause::column("title", SortDirection::Asc)],
        None,
        None,
    );
    assert!(sql.contains("ORDER BY title ASC NULLS LAST"), "got: {sql}");
}

#[test]
fn order_by_nulls_first_flips_null_placement() {
    let dialect = SqliteDialect;
    let descriptor = fixture_descriptor();
    let clause = OrderClause::column("title", SortDirection::Asc).nulls_first();
    let (sql, _) = render_select(&dialect, &descriptor, &[], &[clause], None, None);
    assert!(sql.contains("ORDER BY title ASC NULLS FIRST"), "got: {sql}");
}
