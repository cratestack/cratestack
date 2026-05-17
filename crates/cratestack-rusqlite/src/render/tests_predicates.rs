//! Predicate rendering tests: eq_or_null, coalesce, json.

#![cfg(test)]

use cratestack_sql::{FieldRef, FilterExpr, SqlValue, SqliteDialect};

use super::select::render_select;
use super::tests_fixtures::fixture_descriptor;

#[test]
fn eq_or_null_renders_two_branch_disjunction_with_one_bind() {
    let dialect = SqliteDialect;
    let descriptor = fixture_descriptor();
    let title_eq_or_null = FieldRef::<(), String>::new("title").eq_or_null("hi");
    let (sql, binds) = render_select(
        &dialect,
        &descriptor,
        &[FilterExpr::from(title_eq_or_null)],
        &[],
        None,
        None,
    );
    assert!(
        sql.contains("WHERE (title IS NULL OR title = ?1)"),
        "got: {sql}",
    );
    assert_eq!(binds, vec![SqlValue::String("hi".into())]);
}

#[test]
fn coalesce_lte_renders_coalesce_function_with_single_bind() {
    let dialect = SqliteDialect;
    let descriptor = fixture_descriptor();
    // Bare-&str path:
    let filter = cratestack_sql::coalesce(["title", "published"]).eq("x");
    let (sql, binds) = render_select(&dialect, &descriptor, &[filter], &[], None, None);
    assert!(
        sql.contains("WHERE COALESCE(title, published) = ?1"),
        "got: {sql}",
    );
    assert_eq!(binds, vec![SqlValue::String("x".into())]);
}

#[test]
fn coalesce_accepts_fieldref_items() {
    let dialect = SqliteDialect;
    let descriptor = fixture_descriptor();
    // FieldRef path — exercises the IntoColumnName impl.
    let filter = cratestack_sql::coalesce([
        FieldRef::<(), String>::new("title"),
        FieldRef::<(), String>::new("subtitle"),
    ])
    .is_null();
    let (sql, _) = render_select(&dialect, &descriptor, &[filter], &[], None, None);
    assert!(
        sql.contains("WHERE COALESCE(title, subtitle) IS NULL"),
        "got: {sql}",
    );
}

#[test]
fn json_has_key_lowers_to_json_extract_is_not_null() {
    let dialect = SqliteDialect;
    let descriptor = fixture_descriptor();
    let filter = FieldRef::<(), serde_json::Value>::new("metrics").json_has_key("loss");
    let (sql, binds) = render_select(&dialect, &descriptor, &[filter], &[], None, None);
    assert!(
        sql.contains("WHERE json_extract(metrics, '$.loss') IS NOT NULL"),
        "got: {sql}",
    );
    assert!(binds.is_empty(), "key is inlined, value-less filter");
}

#[test]
fn json_get_text_eq_lowers_to_json_extract_eq() {
    let dialect = SqliteDialect;
    let descriptor = fixture_descriptor();
    let filter = FieldRef::<(), serde_json::Value>::new("metrics")
        .json_get_text("loss")
        .eq("0.001");
    let (sql, binds) = render_select(&dialect, &descriptor, &[filter], &[], None, None);
    assert!(
        sql.contains("WHERE json_extract(metrics, '$.loss') = ?1"),
        "got: {sql}",
    );
    assert_eq!(binds, vec![SqlValue::String("0.001".into())]);
}

#[test]
#[should_panic(expected = "single quote")]
fn json_path_with_single_quote_is_rejected() {
    let dialect = SqliteDialect;
    let descriptor = fixture_descriptor();
    let filter = FieldRef::<(), serde_json::Value>::new("metrics")
        .json_has_key("loss'; DROP TABLE posts;--");
    // Render should panic because the key has a single quote.
    let _ = render_select(&dialect, &descriptor, &[filter], &[], None, None);
}
