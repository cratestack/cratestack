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
            SqlColumnValue {
                column: "title",
                value: SqlValue::String("hi".into()),
            },
            SqlColumnValue {
                column: "published",
                value: SqlValue::Bool(true),
            },
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
        &[SqlColumnValue {
            column: "title",
            value: SqlValue::String("x".into()),
        }],
    );
    let (explicit_sql, _) = render_upsert_with_conflict(
        &dialect,
        &descriptor,
        &[SqlColumnValue {
            column: "title",
            value: SqlValue::String("x".into()),
        }],
        ConflictTarget::PrimaryKey,
    );
    assert_eq!(pk_sql, explicit_sql);
    assert!(pk_sql.contains("ON CONFLICT (id) DO UPDATE SET"));
}

// ───── cratestack#741: partial-index predicate ───────────────────────────

#[test]
fn unpredicated_conflict_target_emits_no_where_clause() {
    // Regression guard: adding the predicate slot must not add a `WHERE`
    // to the unpredicated case's `ON CONFLICT (...)`.
    let dialect = SqliteDialect;
    let descriptor = fixture_descriptor();
    let values = [SqlColumnValue {
        column: "title",
        value: SqlValue::String("hi".into()),
    }];
    let (sql, _) =
        render_upsert_with_conflict(&dialect, &descriptor, &values, ConflictTarget::PRIMARY_KEY);
    assert!(sql.contains("ON CONFLICT (id) DO UPDATE SET"), "got: {sql}");
    assert!(!sql.contains("WHERE"), "got: {sql}");
}

#[test]
fn predicated_conflict_target_emits_where_before_do_update() {
    let dialect = SqliteDialect;
    let descriptor = fixture_descriptor();
    let (sql, _) = render_upsert_with_conflict(
        &dialect,
        &descriptor,
        &[SqlColumnValue {
            column: "title",
            value: SqlValue::String("hi".into()),
        }],
        ConflictTarget::columns(&["title"]).where_index("published = TRUE"),
    );
    assert!(
        sql.contains("ON CONFLICT (title) WHERE published = TRUE DO UPDATE SET"),
        "got: {sql}",
    );
}

#[test]
fn predicate_on_primary_key_target_is_rejected() {
    let target = ConflictTarget::PRIMARY_KEY.where_index("published = TRUE");
    let err = target.validate().expect_err("PK + predicate must error");
    assert!(
        err.to_string().contains("primary key"),
        "error should explain why: {err}",
    );
}

/// `render_upsert_with_conflict` — what `UpsertRecord::preview_sql`
/// calls — deliberately does NOT call `.validate()` (cratestack#741
/// finding 3, mirroring `cratestack_sqlx::UpsertRecord::preview_sql`'s
/// identical decision): it still renders a `WHERE` clause for a target
/// `.validate()` itself rejects, rather than panicking. `.run()` /
/// `.run_in_tx()` are what actually enforce the rejection
/// (`conflict_target_validate` in `delegate/upsert.rs`).
#[test]
fn predicated_primary_key_target_still_renders_a_preview() {
    let dialect = SqliteDialect;
    let descriptor = fixture_descriptor();
    let target = ConflictTarget::PRIMARY_KEY.where_index("published = TRUE");
    assert!(target.validate().is_err(), "sanity: must be invalid");
    let (sql, _) = render_upsert_with_conflict(
        &dialect,
        &descriptor,
        &[SqlColumnValue {
            column: "title",
            value: SqlValue::String("hi".into()),
        }],
        target,
    );
    assert!(
        sql.contains("ON CONFLICT (id) WHERE published = TRUE DO UPDATE SET"),
        "got: {sql}",
    );
}
