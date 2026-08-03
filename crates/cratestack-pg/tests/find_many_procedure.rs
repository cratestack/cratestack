//! Real proof that `FindMany<Model>` procedure arguments produce a real,
//! validated, filtered/sorted query — reusing the exact machinery a
//! `@@paged` model's own generated `list` route already uses, not a
//! separately reimplemented filter grammar.
//!
//! Uses `connect_lazy` (no live Postgres needed, same technique as
//! `tests/schema_fingerprint.rs`): building a `FindMany` query and
//! inspecting its SQL via `.preview_sql()` never touches the database.

use cratestack::include_server_schema;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{CoolError, FindManyInput};

include_server_schema!("tests/fixtures/find_many_procedure.cstack", db = Postgres);

use cratestack_schema::Cratestack;
use cratestack_schema::axum::build_post_query_from_find_many;
use cratestack_schema::models::Post;

fn lazy_db() -> Cratestack {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    Cratestack::builder(pool).build()
}

#[tokio::test]
async fn find_many_input_with_no_filters_previews_a_plain_select() {
    let db = lazy_db();
    let input = FindManyInput::<Post>::default();

    let sql = build_post_query_from_find_many(&db, &input)
        .expect("empty FindMany input should build a query")
        .preview_sql();

    assert!(sql.starts_with("SELECT"), "unexpected SQL: {sql}");
    assert!(!sql.contains("WHERE"), "unexpected WHERE clause: {sql}");
}

#[tokio::test]
async fn find_many_where_clause_reuses_the_real_list_route_filter_grammar() {
    let db = lazy_db();
    let input = FindManyInput::<Post>::new(Some("published=true,authorId=42".to_owned()), None);

    let sql = build_post_query_from_find_many(&db, &input)
        .expect("valid where clause should build a query")
        .preview_sql();

    assert!(sql.contains("WHERE"), "expected a WHERE clause: {sql}");
    assert!(
        sql.contains("published"),
        "expected published filter: {sql}"
    );
    assert!(sql.contains("author_id"), "expected authorId filter: {sql}");
}

#[tokio::test]
async fn find_many_order_by_reuses_the_real_list_route_sort_grammar() {
    let db = lazy_db();
    let input = FindManyInput::<Post>::new(None, Some("-title".to_owned()));

    let sql = build_post_query_from_find_many(&db, &input)
        .expect("valid sort field should build a query")
        .preview_sql();

    assert!(
        sql.contains("ORDER BY"),
        "expected an ORDER BY clause: {sql}"
    );
    assert!(sql.contains("title"), "expected title in ORDER BY: {sql}");
}

#[tokio::test]
async fn find_many_rejects_an_unknown_filter_field_the_same_way_the_list_route_does() {
    let db = lazy_db();
    let input = FindManyInput::<Post>::new(Some("notAField=1".to_owned()), None);

    match build_post_query_from_find_many(&db, &input) {
        Ok(_) => panic!("unknown filter field should be rejected"),
        Err(error) => assert!(
            matches!(error, CoolError::BadRequest(_)),
            "expected a BadRequest error, got: {error:?}"
        ),
    }
}

#[tokio::test]
async fn find_many_rejects_malformed_where_syntax() {
    let db = lazy_db();
    let input = FindManyInput::<Post>::new(Some("this is not valid".to_owned()), None);

    assert!(build_post_query_from_find_many(&db, &input).is_err());
}
