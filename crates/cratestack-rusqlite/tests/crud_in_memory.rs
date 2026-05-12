//! Full CRUD round-trip against in-memory SQLite, without involving any
//! macro-generated code. Defines `Post` as a hand-written model + descriptor
//! + create/update inputs, then exercises every delegate verb.
//!
//! This is the contract test for the on-device storage layer: anything
//! `include_embedded_schema!(...)` emits must satisfy the same
//! interfaces this file uses by hand.

use cratestack_rusqlite::{
    CreateModelInput, FromRusqliteRow, ModelDelegate, RusqliteRuntime, SqlColumnValue, SqlValue,
    UpdateModelInput, ddl::create_table_sql,
};
use cratestack_sql::{FieldRef, ModelColumn, ModelDescriptor};
use rusqlite::Row;

#[derive(Debug, Clone, PartialEq)]
struct Post {
    id: i64,
    title: String,
    published: bool,
}

impl FromRusqliteRow for Post {
    fn from_rusqlite_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            title: row.get("title")?,
            published: row.get::<_, i64>("published")? != 0,
        })
    }
}

#[derive(Debug, Clone)]
struct CreatePostInput {
    title: String,
    published: bool,
}

impl CreateModelInput<Post> for CreatePostInput {
    fn sql_values(&self) -> Vec<SqlColumnValue> {
        vec![
            SqlColumnValue {
                column: "title",
                value: SqlValue::String(self.title.clone()),
            },
            SqlColumnValue {
                column: "published",
                value: SqlValue::Bool(self.published),
            },
        ]
    }
}

#[derive(Debug, Clone, Default)]
struct UpdatePostInput {
    title: Option<String>,
    published: Option<bool>,
}

impl UpdateModelInput<Post> for UpdatePostInput {
    fn sql_values(&self) -> Vec<SqlColumnValue> {
        let mut values = Vec::new();
        if let Some(title) = &self.title {
            values.push(SqlColumnValue {
                column: "title",
                value: SqlValue::String(title.clone()),
            });
        }
        if let Some(published) = self.published {
            values.push(SqlColumnValue {
                column: "published",
                value: SqlValue::Bool(published),
            });
        }
        values
    }
}

const COLUMNS: &[ModelColumn] = &[
    ModelColumn {
        rust_name: "id",
        sql_name: "id",
    },
    ModelColumn {
        rust_name: "title",
        sql_name: "title",
    },
    ModelColumn {
        rust_name: "published",
        sql_name: "published",
    },
];

static POST_DESCRIPTOR: ModelDescriptor<Post, i64> = ModelDescriptor::new(
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
);

fn setup() -> RusqliteRuntime {
    let runtime = RusqliteRuntime::open_in_memory().expect("open in-memory sqlite");
    let mut ddl = create_table_sql(&POST_DESCRIPTOR);
    // The hand-written test fixture uses an `INTEGER PRIMARY KEY` so SQLite
    // auto-rowid-aliases it. The generic DDL produces `id BLOB PRIMARY KEY`
    // (BLOB affinity is correct for the codegen path — see ddl.rs — but
    // doesn't trigger rowid aliasing). Patch the PK column type just for
    // this auto-increment test.
    ddl = ddl.replace("id BLOB PRIMARY KEY", "id INTEGER PRIMARY KEY");
    runtime
        .with_connection(|conn| {
            conn.execute_batch(&ddl).expect("apply DDL");
            Ok(())
        })
        .unwrap();
    runtime
}

#[test]
fn create_returns_full_row_with_assigned_pk() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &POST_DESCRIPTOR);
    let created = delegate
        .create(CreatePostInput {
            title: "first".into(),
            published: true,
        })
        .run()
        .expect("create succeeds");
    // SQLite assigned an auto-increment rowid for the INTEGER PRIMARY KEY;
    // we don't care about the exact value, only that it round-trips.
    assert!(created.id > 0);
    assert_eq!(created.title, "first");
    assert!(created.published);
}

#[test]
fn find_unique_returns_inserted_row() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &POST_DESCRIPTOR);
    let created = delegate
        .create(CreatePostInput {
            title: "hello".into(),
            published: false,
        })
        .run()
        .unwrap();
    let fetched = delegate
        .find_unique(created.id)
        .run()
        .expect("find_unique succeeds")
        .expect("row exists");
    assert_eq!(fetched, created);
}

#[test]
fn find_many_filters_and_orders() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &POST_DESCRIPTOR);
    for title in ["alpha", "beta", "gamma"] {
        delegate
            .create(CreatePostInput {
                title: title.into(),
                published: title != "beta",
            })
            .run()
            .unwrap();
    }

    let published = FieldRef::<Post, bool>::new("published");
    let title = FieldRef::<Post, String>::new("title");
    let results: Vec<Post> = delegate
        .find_many()
        .where_(published.is_true())
        .order_by(title.asc())
        .run()
        .expect("find_many succeeds");

    assert_eq!(results.iter().map(|p| p.title.as_str()).collect::<Vec<_>>(), ["alpha", "gamma"]);
}

#[test]
fn update_overwrites_specified_columns_only() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &POST_DESCRIPTOR);
    let created = delegate
        .create(CreatePostInput {
            title: "original".into(),
            published: false,
        })
        .run()
        .unwrap();
    let updated = delegate
        .update(created.id)
        .set(UpdatePostInput {
            title: Some("edited".into()),
            published: None,
        })
        .run()
        .expect("update succeeds");
    assert_eq!(updated.title, "edited");
    assert!(!updated.published, "published must remain false");
}

#[test]
fn delete_removes_row_from_table() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &POST_DESCRIPTOR);
    let created = delegate
        .create(CreatePostInput {
            title: "doomed".into(),
            published: true,
        })
        .run()
        .unwrap();
    let deleted = delegate.delete(created.id).run().expect("delete succeeds");
    assert_eq!(deleted, created);
    let after = delegate.find_unique(created.id).run().unwrap();
    assert!(after.is_none(), "row should be gone");
}

#[test]
fn limit_and_offset_paginate_correctly() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &POST_DESCRIPTOR);
    for i in 0..5 {
        delegate
            .create(CreatePostInput {
                title: format!("post-{i}"),
                published: true,
            })
            .run()
            .unwrap();
    }
    let title = FieldRef::<Post, String>::new("title");
    let page: Vec<Post> = delegate
        .find_many()
        .order_by(title.asc())
        .limit(2)
        .offset(2)
        .run()
        .unwrap();
    assert_eq!(
        page.iter().map(|p| p.title.clone()).collect::<Vec<_>>(),
        vec!["post-2".to_string(), "post-3".to_string()],
    );
}

#[test]
fn preview_sql_matches_runtime_path_placeholders() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &POST_DESCRIPTOR);
    let title = FieldRef::<Post, String>::new("title");
    let sql = delegate
        .find_many()
        .where_(title.eq("x"))
        .limit(10)
        .preview_sql();
    assert!(sql.contains("?1"), "filter placeholder missing: {sql}");
    assert!(sql.contains("?2"), "limit placeholder missing: {sql}");
}
