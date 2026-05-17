//! INSERT / UPDATE / DELETE / *_many rendering tests.

#![cfg(test)]

use cratestack_sql::{FieldRef, FilterExpr, SqlColumnValue, SqlValue, SqliteDialect};

use super::delete::render_delete;
use super::delete_many::render_delete_many;
use super::insert::render_insert;
use super::tests_fixtures::{fixture_descriptor, soft_delete_descriptor};
use super::update::render_update;
use super::update_many::render_update_many;

#[test]
fn insert_returns_full_row_projection() {
    let dialect = SqliteDialect;
    let descriptor = fixture_descriptor();
    let (sql, binds) = render_insert(
        &dialect,
        &descriptor,
        &[
            SqlColumnValue {
                column: "title",
                value: SqlValue::String("hi".into()),
            },
            SqlColumnValue {
                column: "published",
                value: SqlValue::Bool(true),
            },
        ],
    );
    assert!(sql.starts_with("INSERT INTO posts (title, published) VALUES (?1, ?2)"));
    assert!(sql.contains("RETURNING"));
    assert_eq!(
        binds,
        vec![SqlValue::String("hi".into()), SqlValue::Bool(true)],
    );
}

#[test]
fn update_binds_id_last_and_returns_row() {
    let dialect = SqliteDialect;
    let descriptor = fixture_descriptor();
    let (sql, binds) = render_update(
        &dialect,
        &descriptor,
        &[SqlColumnValue {
            column: "title",
            value: SqlValue::String("new".into()),
        }],
        SqlValue::Int(5),
    );
    assert!(sql.starts_with("UPDATE posts SET title = ?1 WHERE id = ?2"));
    assert!(sql.contains("RETURNING"));
    assert_eq!(
        binds,
        vec![SqlValue::String("new".into()), SqlValue::Int(5)]
    );
}

#[test]
fn delete_hard_emits_delete_statement() {
    let dialect = SqliteDialect;
    let descriptor = fixture_descriptor();
    let (sql, binds) = render_delete(&dialect, &descriptor, SqlValue::Int(9), chrono::Utc::now());
    assert!(sql.starts_with("DELETE FROM posts WHERE id = ?1 RETURNING"));
    assert_eq!(binds, vec![SqlValue::Int(9)]);
}

#[test]
fn delete_soft_emits_update_of_deleted_at() {
    let dialect = SqliteDialect;
    let descriptor = soft_delete_descriptor();
    let now = chrono::Utc::now();
    let (sql, binds) = render_delete(&dialect, &descriptor, SqlValue::Int(9), now);
    assert!(sql.starts_with("UPDATE posts SET deleted_at = ?1 WHERE id = ?2"));
    assert_eq!(binds, vec![SqlValue::DateTime(now), SqlValue::Int(9)]);
}

#[test]
fn update_many_with_filter_renders_set_and_where() {
    let dialect = SqliteDialect;
    let descriptor = fixture_descriptor();
    let title_filter = FieldRef::<(), String>::new("title").eq("foo");
    let (sql, binds) = render_update_many(
        &dialect,
        &descriptor,
        &[SqlColumnValue {
            column: "published",
            value: SqlValue::Bool(true),
        }],
        &[FilterExpr::from(title_filter)],
    );
    assert_eq!(
        sql,
        "UPDATE posts SET published = ?1 WHERE (title = ?2) RETURNING id AS \"id\", title AS \"title\", published AS \"published\"",
    );
    assert_eq!(
        binds,
        vec![SqlValue::Bool(true), SqlValue::String("foo".into())],
    );
}

#[test]
fn update_many_with_soft_delete_layers_in_isnull_clause() {
    let dialect = SqliteDialect;
    let descriptor = soft_delete_descriptor();
    let (sql, _) = render_update_many(
        &dialect,
        &descriptor,
        &[SqlColumnValue {
            column: "title",
            value: SqlValue::String("renamed".into()),
        }],
        &[FilterExpr::from(
            FieldRef::<(), bool>::new("published").is_true(),
        )],
    );
    assert!(sql.contains("WHERE deleted_at IS NULL AND ("), "got: {sql}");
}

#[test]
fn delete_many_hard_emits_delete_with_filter_predicate() {
    let dialect = SqliteDialect;
    let descriptor = fixture_descriptor();
    let title_filter = FieldRef::<(), String>::new("title").eq("doomed");
    let (sql, binds) = render_delete_many(&dialect, &descriptor, &[FilterExpr::from(title_filter)]);
    assert!(
        sql.starts_with("DELETE FROM posts WHERE (title = ?1)"),
        "got: {sql}",
    );
    assert!(sql.contains("RETURNING"));
    assert_eq!(binds, vec![SqlValue::String("doomed".into())]);
}

#[test]
fn delete_many_soft_delete_emits_update_of_deleted_at() {
    let dialect = SqliteDialect;
    let descriptor = soft_delete_descriptor();
    let id_filter = FieldRef::<(), i64>::new("id").gte(10i64);
    let (sql, _) = render_delete_many(&dialect, &descriptor, &[FilterExpr::from(id_filter)]);
    assert!(
        sql.contains("UPDATE posts SET deleted_at = CURRENT_TIMESTAMP"),
        "got: {sql}",
    );
    assert!(sql.contains("WHERE deleted_at IS NULL AND ("));
}
