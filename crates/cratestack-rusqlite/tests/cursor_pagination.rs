//! Keyset (cursor) paging on the embedded rusqlite backend (cratestack#1304):
//! the bound `keyset_after` builds is an ordinary `FilterExpr`, so paging stays
//! stable while rows are inserted between page fetches — which offset paging
//! over the same data does not.

use cratestack_core::{CratestackError, CursorPage, decode_cursor, encode_cursor};
use cratestack_rusqlite::{
    CreateModelInput, FromRusqliteRow, ModelDelegate, RusqliteRuntime, SqlColumnValue, SqlValue,
    ddl::create_table_sql,
};
use cratestack_sql::{
    FieldRef, FilterExpr, KeysetLeaf, ModelColumn, ModelDescriptor, keyset_after,
    keyset_order_spec, parse_keyset_sort,
};
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

fn create(runtime: &RusqliteRuntime, title: &str) {
    ModelDelegate::new(runtime, &POST_DESCRIPTOR)
        .create(CreatePostInput {
            title: title.to_owned(),
            published: true,
        })
        .run()
        .expect("create succeeds");
}

fn leaf(column: &str, leaf: KeysetLeaf<'_, String>) -> Result<FilterExpr, CratestackError> {
    Ok(match (column, leaf) {
        ("title", KeysetLeaf::Eq(v)) => FieldRef::<Post, String>::new("title").eq(v.clone()).into(),
        ("title", KeysetLeaf::Gt(v)) => FieldRef::<Post, String>::new("title").gt(v.clone()).into(),
        ("title", KeysetLeaf::Lt(v)) => FieldRef::<Post, String>::new("title").lt(v.clone()).into(),
        ("id", KeysetLeaf::Eq(v)) => FieldRef::<Post, i64>::new("id")
            .eq(v.parse::<i64>().unwrap())
            .into(),
        ("id", KeysetLeaf::Gt(v)) => FieldRef::<Post, i64>::new("id")
            .gt(v.parse::<i64>().unwrap())
            .into(),
        ("id", KeysetLeaf::Lt(v)) => FieldRef::<Post, i64>::new("id")
            .lt(v.parse::<i64>().unwrap())
            .into(),
        other => return Err(CratestackError::BadRequest(format!("{other:?}"))),
    })
}

/// One cursor page over `sort` (`title` ascending then `id`), the way a
/// hand-written embedded caller composes the primitives.
fn page(runtime: &RusqliteRuntime, cursor: Option<&str>, limit: i64) -> CursorPage<Post> {
    let columns = parse_keyset_sort(Some("title"), "id", |name| {
        matches!(name, "title" | "id").then_some(false)
    })
    .unwrap();
    let order = keyset_order_spec(&columns);
    let mut find = ModelDelegate::new(runtime, &POST_DESCRIPTOR)
        .find_many()
        .order_by(FieldRef::<Post, String>::new("title").asc())
        .order_by(FieldRef::<Post, i64>::new("id").asc())
        .limit(limit + 1);
    if let Some(cursor) = cursor {
        let values = decode_cursor(cursor, &order, columns.len()).expect("valid cursor");
        find = find.where_expr(keyset_after(&columns, &values, leaf).expect("keyset bound"));
    }
    CursorPage::from_rows(find.run().expect("query"), limit, |last| {
        encode_cursor(
            &order,
            &[Some(last.title.clone()), Some(last.id.to_string())],
        )
    })
}

fn titles(page: &CursorPage<Post>) -> Vec<&str> {
    page.items.iter().map(|post| post.title.as_str()).collect()
}

#[test]
fn keyset_paging_is_stable_across_inserts_between_pages() {
    let runtime = setup();
    for title in ["b", "d", "d", "f", "h"] {
        create(&runtime, title);
    }

    let first = page(&runtime, None, 2);
    assert_eq!(titles(&first), ["b", "d"]);
    let cursor = first.next_cursor.clone().expect("more rows remain");

    // Rows land both before and after the cursor while the client holds it.
    create(&runtime, "a");
    create(&runtime, "d"); // ties the cursor row's title, later id
    create(&runtime, "g");

    let second = page(&runtime, Some(&cursor), 2);
    assert_eq!(
        titles(&second),
        ["d", "d"],
        "no repeat of b/d, tie rows kept"
    );
    let third = page(&runtime, second.next_cursor.as_deref(), 10);
    assert_eq!(titles(&third), ["f", "g", "h"]);
    assert_eq!(third.next_cursor, None);

    // The same sequence with offset paging repeats a row ("a" shifted everything).
    let offset_second = ModelDelegate::new(&runtime, &POST_DESCRIPTOR)
        .find_many()
        .order_by(FieldRef::<Post, String>::new("title").asc())
        .order_by(FieldRef::<Post, i64>::new("id").asc())
        .limit(2)
        .offset(2)
        .run()
        .unwrap();
    assert_eq!(
        offset_second[0].id, first.items[1].id,
        "offset paging re-serves page 1's last row after a front insert"
    );
}

#[test]
fn cursor_from_another_order_is_rejected_by_decode() {
    let cursor = encode_cursor("-title,id", &[Some("b".to_owned()), Some("1".to_owned())]);
    assert!(matches!(
        decode_cursor(&cursor, "title,id", 2),
        Err(CratestackError::BadRequest(_))
    ));
}
