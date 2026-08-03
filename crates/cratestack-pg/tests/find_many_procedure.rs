//! Real proof that `FindMany<Model>` procedure arguments produce a real,
//! validated, filtered/sorted query — the generated `PostWhere`/
//! `PostOrderByClause` types call straight into the model's own
//! `FieldRef` accessors (the same ones the untyped REST `?where=` route
//! uses), so there's zero duplicated filter logic and zero possibility
//! of an "unknown field" or "malformed syntax" error at all: unlike the
//! old raw-string design, both are ruled out at compile time by the
//! generated struct's own fields.
//!
//! Uses `connect_lazy` (no live Postgres needed, same technique as
//! `tests/schema_fingerprint.rs`): building a `FindMany` query and
//! inspecting its SQL via `.preview_sql()` never touches the database.

use cratestack::include_server_schema;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{FieldFilterInput, SortDirection};

include_server_schema!("tests/fixtures/find_many_procedure.cstack", db = Postgres);

use cratestack_schema::Cratestack;
use cratestack_schema::{
    PostFindManyInput, PostOrderByClause, PostSortField, PostWhere, build_post_query_from_find_many,
};

fn lazy_db() -> Cratestack {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    Cratestack::builder(pool).build()
}

#[tokio::test]
async fn find_many_input_with_no_filters_previews_a_plain_select() {
    let db = lazy_db();
    let input = PostFindManyInput::default();

    let sql = build_post_query_from_find_many(&db, &input).preview_sql();

    assert!(sql.starts_with("SELECT"), "unexpected SQL: {sql}");
    assert!(!sql.contains("WHERE"), "unexpected WHERE clause: {sql}");
}

#[tokio::test]
async fn find_many_where_reuses_the_same_field_refs_the_list_route_uses() {
    let db = lazy_db();
    let input = PostFindManyInput {
        r#where: Some(PostWhere {
            published: Some(FieldFilterInput {
                eq: Some(true),
                ..Default::default()
            }),
            authorId: Some(FieldFilterInput {
                gt: Some(10),
                ..Default::default()
            }),
            title: Some(FieldFilterInput {
                contains: Some("hello".to_owned()),
                ..Default::default()
            }),
            ..Default::default()
        }),
        order_by: None,
    };

    let sql = build_post_query_from_find_many(&db, &input).preview_sql();

    assert!(sql.contains("WHERE"), "expected a WHERE clause: {sql}");
    assert!(
        sql.contains("published"),
        "expected published filter: {sql}"
    );
    assert!(
        sql.contains("author_id"),
        "expected author_id filter: {sql}"
    );
    assert!(sql.contains("title"), "expected title filter: {sql}");
    assert!(
        sql.contains("LIKE"),
        "expected contains to render LIKE: {sql}"
    );
}

#[tokio::test]
async fn find_many_where_is_null_targets_the_optional_field() {
    let db = lazy_db();
    let input = PostFindManyInput {
        r#where: Some(PostWhere {
            subtitle: Some(FieldFilterInput {
                is_null: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        }),
        order_by: None,
    };

    let sql = build_post_query_from_find_many(&db, &input).preview_sql();

    assert!(sql.contains("subtitle"), "expected subtitle filter: {sql}");
    assert!(sql.contains("IS NULL"), "expected IS NULL: {sql}");
}

#[tokio::test]
async fn find_many_order_by_reuses_the_same_field_refs_multi_key() {
    let db = lazy_db();
    let input = PostFindManyInput {
        r#where: None,
        order_by: Some(vec![
            PostOrderByClause {
                field: PostSortField::Published,
                direction: SortDirection::Desc,
            },
            PostOrderByClause {
                field: PostSortField::Title,
                direction: SortDirection::Asc,
            },
        ]),
    };

    let sql = build_post_query_from_find_many(&db, &input).preview_sql();

    assert!(
        sql.contains("ORDER BY"),
        "expected an ORDER BY clause: {sql}"
    );
    // Multi-key order must be preserved: published DESC first, title ASC
    // second — this is exactly the ordering guarantee a field-keyed JSON
    // object (rather than this typed `Vec<PostOrderByClause>`) could
    // silently lose. Search only within the ORDER BY clause itself (the
    // SELECT column list earlier in the SQL also contains "published",
    // which would otherwise false-positive this assertion).
    let order_by_index = sql.find("ORDER BY").expect("has ORDER BY");
    let order_by_clause = &sql[order_by_index..];
    assert_eq!(
        order_by_clause, "ORDER BY published DESC NULLS LAST, title ASC NULLS LAST",
        "unexpected ORDER BY clause: {order_by_clause}"
    );
}
