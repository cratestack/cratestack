//! Round-trip tests for the new builder verbs: `.update_many()`,
//! `.run_in_tx()` on CRUD writes, and `.for_update()` on reads.
//!
//! Uses the same hand-rolled `Post` fixture as `crud_in_memory.rs` so the
//! schema-DDL side stays unchanged and the focus is on builder behaviour.

use cratestack_core::BatchSummary;
use cratestack_rusqlite::{
    CreateModelInput, FromRusqliteRow, ModelDelegate, RusqliteError, RusqliteRuntime,
    SqlColumnValue, SqlValue, UpdateModelInput, ddl::create_table_sql,
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
    &[],
);

fn setup() -> RusqliteRuntime {
    let runtime = RusqliteRuntime::open_in_memory().expect("open in-memory sqlite");
    let mut ddl = create_table_sql(&POST_DESCRIPTOR);
    ddl = ddl.replace("id BLOB PRIMARY KEY", "id INTEGER PRIMARY KEY");
    runtime
        .with_connection(|conn| {
            conn.execute_batch(&ddl).expect("apply DDL");
            Ok(())
        })
        .unwrap();
    runtime
}

fn seed(delegate: &ModelDelegate<'_, Post, i64>, titles_published: &[(&str, bool)]) -> Vec<Post> {
    titles_published
        .iter()
        .map(|(title, published)| {
            delegate
                .create(CreatePostInput {
                    title: (*title).into(),
                    published: *published,
                })
                .run()
                .unwrap()
        })
        .collect()
}

// ───── #1 update_many ────────────────────────────────────────────────────────

#[test]
fn update_many_mutates_only_rows_matching_filter() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &POST_DESCRIPTOR);
    let _ = seed(
        &delegate,
        &[("alpha", true), ("beta", false), ("gamma", true)],
    );

    let published = FieldRef::<Post, bool>::new("published");
    let summary: BatchSummary = delegate
        .update_many()
        .where_(published.is_true())
        .set(UpdatePostInput {
            title: Some("renamed".into()),
            published: None,
        })
        .run()
        .expect("update_many succeeds");

    assert_eq!(summary.total, 2);
    assert_eq!(summary.ok, 2);
    assert_eq!(summary.err, 0);

    // Verify exactly the two published rows were renamed.
    let renamed: Vec<Post> = delegate
        .find_many()
        .where_(FieldRef::<Post, String>::new("title").eq("renamed"))
        .run()
        .unwrap();
    assert_eq!(renamed.len(), 2);
    let beta: Vec<Post> = delegate
        .find_many()
        .where_(FieldRef::<Post, String>::new("title").eq("beta"))
        .run()
        .unwrap();
    assert_eq!(beta.len(), 1, "beta must be untouched");
}

#[test]
fn update_many_refuses_to_run_without_filter() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &POST_DESCRIPTOR);
    let _ = seed(&delegate, &[("a", true), ("b", true)]);

    let err = delegate
        .update_many()
        .set(UpdatePostInput {
            title: Some("nope".into()),
            published: None,
        })
        .run()
        .expect_err("predicate-less update_many must be rejected");
    assert!(matches!(err, RusqliteError::Validation(_)), "got {err:?}");
}

#[test]
fn update_many_empty_match_returns_zero_summary() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &POST_DESCRIPTOR);
    let _ = seed(&delegate, &[("a", false), ("b", false)]);

    let published = FieldRef::<Post, bool>::new("published");
    let summary = delegate
        .update_many()
        .where_(published.is_true())
        .set(UpdatePostInput {
            title: Some("nope".into()),
            published: None,
        })
        .run()
        .expect("update_many succeeds");
    assert_eq!(summary.total, 0);
    assert_eq!(summary.ok, 0);
}

// ───── #2 .run_in_tx() ───────────────────────────────────────────────────────

#[test]
fn run_in_tx_commits_when_caller_commits() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &POST_DESCRIPTOR);

    let created = runtime
        .with_connection(|conn| {
            let tx = conn.transaction()?;
            let row = delegate
                .create(CreatePostInput {
                    title: "in-tx".into(),
                    published: true,
                })
                .run_in_tx(&tx)
                .map_err(|e| match e {
                    RusqliteError::Sqlite(err) => err,
                    _ => rusqlite::Error::InvalidQuery,
                })?;
            tx.commit()?;
            let _ = row.id;
            Ok(row)
        })
        .expect("tx commit succeeds");

    // Row must be visible after the tx commit.
    let found = delegate.find_unique(created.id).run().unwrap();
    assert_eq!(found, Some(created));
}

#[test]
fn run_in_tx_rolls_back_when_caller_rolls_back() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &POST_DESCRIPTOR);

    let id = runtime
        .with_connection(|conn| {
            let tx = conn.transaction()?;
            let row = delegate
                .create(CreatePostInput {
                    title: "doomed".into(),
                    published: false,
                })
                .run_in_tx(&tx)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            // Intentionally drop without committing → rollback.
            drop(tx);
            Ok(row.id)
        })
        .expect("with_connection succeeds");

    let found = delegate.find_unique(id).run().unwrap();
    assert!(found.is_none(), "row must not be visible after rollback");
}

// ───── #5 .for_update() ──────────────────────────────────────────────────────

#[test]
fn for_update_is_a_noop_on_embedded_runtime() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &POST_DESCRIPTOR);
    let post = seed(&delegate, &[("only", true)]).pop().unwrap();

    // `.for_update()` on the embedded delegate is API-compat only; calling
    // it should not alter the SQL or the result.
    let found = delegate
        .find_unique(post.id)
        .for_update()
        .run()
        .unwrap()
        .expect("row exists");
    assert_eq!(found, post);

    let listed = delegate
        .find_many()
        .where_(FieldRef::<Post, bool>::new("published").is_true())
        .for_update()
        .run()
        .unwrap();
    assert_eq!(listed.len(), 1);
}
