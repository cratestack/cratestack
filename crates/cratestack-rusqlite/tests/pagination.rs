//! `FindMany::paginate` — real `COUNT(*)` + paginated `SELECT` assembled
//! into a `Page<M>`/`PageInfo`, available unconditionally on every
//! model's delegate (no `@@paged` gate needed on the embedded backend —
//! see `include/embedded.rs`'s module doc for why).
//!
//! Uses the same hand-rolled `Post` fixture as `crud_in_memory.rs`.

use cratestack_core::PageInput;
use cratestack_rusqlite::{
    CreateModelInput, FromRusqliteRow, ModelDelegate, RusqliteRuntime, SqlColumnValue, SqlValue,
    ddl::create_table_sql,
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

fn seed(runtime: &RusqliteRuntime, count: usize) {
    let delegate = ModelDelegate::new(runtime, &POST_DESCRIPTOR);
    for i in 0..count {
        delegate
            .create(CreatePostInput {
                title: format!("post-{i}"),
                published: i % 2 == 0,
            })
            .run()
            .expect("seed create succeeds");
    }
}

#[test]
fn paginate_returns_real_total_count_and_page_info() {
    let runtime = setup();
    seed(&runtime, 5);
    let delegate = ModelDelegate::new(&runtime, &POST_DESCRIPTOR);

    let page = delegate
        .find_many()
        .paginate(PageInput {
            limit: Some(2),
            offset: Some(0),
        })
        .expect("paginate succeeds");

    assert_eq!(page.items.len(), 2);
    assert_eq!(page.total_count, Some(5));
    assert_eq!(page.page_info.limit, Some(2));
    assert_eq!(page.page_info.offset, Some(0));
    assert!(page.page_info.has_next_page);
    assert!(!page.page_info.has_previous_page);
}

#[test]
fn paginate_last_page_has_no_next_page() {
    let runtime = setup();
    seed(&runtime, 5);
    let delegate = ModelDelegate::new(&runtime, &POST_DESCRIPTOR);

    let page = delegate
        .find_many()
        .paginate(PageInput {
            limit: Some(2),
            offset: Some(4),
        })
        .expect("paginate succeeds");

    assert_eq!(page.items.len(), 1);
    assert_eq!(page.total_count, Some(5));
    assert!(!page.page_info.has_next_page);
    assert!(page.page_info.has_previous_page);
}

#[test]
fn paginate_honors_where_filters_in_both_count_and_select() {
    let runtime = setup();
    seed(&runtime, 5);
    let delegate = ModelDelegate::new(&runtime, &POST_DESCRIPTOR);

    let published_field = FieldRef::<Post, bool>::new("published");
    let page = delegate
        .find_many()
        .where_(published_field.eq(true))
        .paginate(PageInput {
            limit: Some(10),
            offset: Some(0),
        })
        .expect("paginate succeeds");

    // Seeded rows 0, 2, 4 are published (i % 2 == 0).
    assert_eq!(page.total_count, Some(3));
    assert_eq!(page.items.len(), 3);
    assert!(page.items.iter().all(|post| post.published));
}

#[test]
fn paginate_defaults_limit_to_max_list_limit_when_unset() {
    let runtime = setup();
    seed(&runtime, 3);
    let delegate = ModelDelegate::new(&runtime, &POST_DESCRIPTOR);

    let page = delegate
        .find_many()
        .paginate(PageInput::default())
        .expect("paginate succeeds");

    assert_eq!(page.items.len(), 3);
    assert_eq!(page.page_info.limit, Some(cratestack_core::MAX_LIST_LIMIT));
    assert_eq!(page.page_info.offset, Some(0));
}
