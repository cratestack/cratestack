use std::sync::Arc;

use cratestack_core::Schema;
use rusqlite::Connection;

use super::SqliteSource;
use super::sql::{build_json_object, build_list_sql};
use crate::data::PageRequest;
use crate::data::model_info::{PkCast, resolve_model};

fn parse(text: &str) -> Schema {
    cratestack_parser::parse_schema(text).expect("schema parses")
}

fn make_source(schema: Schema) -> SqliteSource {
    let conn = Connection::open_in_memory().expect("open sqlite");
    conn.execute_batch(
        r#"
        CREATE TABLE customers (id INTEGER PRIMARY KEY, email TEXT NOT NULL);
        INSERT INTO customers (id, email) VALUES
          (1, 'a@example.com'),
          (2, 'b@example.com'),
          (3, 'c@example.com');

        CREATE TABLE posts (
          id TEXT PRIMARY KEY,
          author_id INTEGER NOT NULL,
          title TEXT NOT NULL
        );
        INSERT INTO posts (id, author_id, title) VALUES
          ('p1', 1, 'first'),
          ('p2', 1, 'second'),
          ('p3', 2, 'third');
        "#,
    )
    .expect("ddl");
    SqliteSource::new(conn, Arc::new(schema))
}

const BLOG_SCHEMA: &str = r#"
    model Customer {
      id Int @id
      email String
      posts Post[] @relation(fields: [id], references: [authorId])
    }

    model Post {
      id String @id
      authorId Int
      title String
      author Customer @relation(fields: [authorId], references: [id])
    }
"#;

#[test]
fn list_sql_uses_text_cursor_for_string_pk() {
    let schema = parse(BLOG_SCHEMA);
    let (_, info) = resolve_model(&schema, "Post").unwrap();
    let sql = build_list_sql(&info, 25);
    assert!(sql.contains(r#""id" > ?1"#), "{sql}");
    assert!(!sql.contains("CAST"), "{sql}");
    assert!(sql.contains("LIMIT 25"), "{sql}");
    assert!(sql.contains(r#"FROM "posts""#), "{sql}");
}

#[test]
fn list_sql_casts_int_pk_via_cast_as_integer() {
    let schema = parse(BLOG_SCHEMA);
    let (_, info) = resolve_model(&schema, "Customer").unwrap();
    let sql = build_list_sql(&info, 10);
    assert!(sql.contains(r#""id" > CAST(?1 AS INTEGER)"#), "{sql}");
}

#[test]
fn json_object_emits_field_name_aliases() {
    let schema = parse(BLOG_SCHEMA);
    let (_, info) = resolve_model(&schema, "Post").unwrap();
    let object = build_json_object(&info);
    assert!(object.contains("'id', \"id\""), "{object}");
    assert!(object.contains("'authorId', \"author_id\""), "{object}");
    assert!(object.contains("'title', \"title\""), "{object}");
}

#[tokio::test]
async fn list_returns_rows_with_field_name_aliases() {
    use crate::data::DataSource;
    let schema = parse(BLOG_SCHEMA);
    let source = make_source(schema);
    let page = source
        .list("Post", PageRequest::default())
        .await
        .expect("list ok");
    assert_eq!(page.rows.len(), 3);
    let first = &page.rows[0];
    assert_eq!(first.get("id").unwrap(), &serde_json::json!("p1"));
    assert_eq!(first.get("authorId").unwrap(), &serde_json::json!(1));
    assert_eq!(first.get("title").unwrap(), &serde_json::json!("first"));
}

#[tokio::test]
async fn list_paginates_with_cursor() {
    use crate::data::DataSource;
    let schema = parse(BLOG_SCHEMA);
    let source = make_source(schema);
    let page1 = source
        .list(
            "Post",
            PageRequest {
                cursor: None,
                limit: Some(2),
            },
        )
        .await
        .expect("first page");
    assert_eq!(page1.rows.len(), 2);
    assert_eq!(page1.next_cursor.as_deref(), Some("p2"));

    let page2 = source
        .list(
            "Post",
            PageRequest {
                cursor: Some("p2"),
                limit: Some(2),
            },
        )
        .await
        .expect("second page");
    assert_eq!(page2.rows.len(), 1);
    assert!(page2.next_cursor.is_none());
}

#[tokio::test]
async fn get_returns_single_row_by_pk() {
    use crate::data::DataSource;
    let schema = parse(BLOG_SCHEMA);
    let source = make_source(schema);
    let row = source.get("Post", "p2").await.expect("get").expect("some");
    assert_eq!(row.get("title").unwrap(), &serde_json::json!("second"));
}

#[tokio::test]
async fn get_returns_none_for_missing_pk() {
    use crate::data::DataSource;
    let schema = parse(BLOG_SCHEMA);
    let source = make_source(schema);
    let row = source.get("Post", "missing").await.expect("get ok");
    assert!(row.is_none());
}

#[tokio::test]
async fn follow_returns_rows_matching_filter_column() {
    use crate::data::DataSource;
    let schema = parse(BLOG_SCHEMA);
    let source = make_source(schema);
    let page = source
        .follow(
            "Post",
            "author_id",
            PkCast::BigInt,
            "1",
            PageRequest::default(),
        )
        .await
        .expect("follow ok");
    assert_eq!(page.rows.len(), 2);
    let titles: Vec<&str> = page
        .rows
        .iter()
        .filter_map(|r| r.get("title").and_then(|v| v.as_str()))
        .collect();
    assert!(titles.contains(&"first"));
    assert!(titles.contains(&"second"));
}
