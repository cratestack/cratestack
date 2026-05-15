//! Round-trip tests for the tier-3 filter primitives:
//!
//!   * `FieldRef::eq_or_null(v)` — matches rows where the column is
//!     either null or equals `v`.
//!   * `FieldRef::match_optional(opt)` — `None` skips the filter
//!     entirely; `Some(v)` falls through to `eq_or_null`.
//!   * `coalesce([col_a, col_b, ...]).<op>(value)` — compares the
//!     first non-null among the listed columns against `value`.
//!   * `.where_optional(maybe_filter)` — builder sugar for skipping
//!     filters that resolved to `None`.

use cratestack_rusqlite::{
    coalesce, CreateModelInput, FromRusqliteRow, ModelDelegate, RusqliteRuntime, SqlColumnValue,
    SqlValue, UpdateModelInput,
};
use cratestack_sql::{FieldRef, ModelColumn, ModelDescriptor};
use rusqlite::Row;

#[derive(Debug, Clone, PartialEq)]
struct Task {
    id: i64,
    market_code: Option<String>,
    next_attempt_at: Option<i64>,
    scheduled_at: Option<i64>,
    created_at: i64,
}

impl FromRusqliteRow for Task {
    fn from_rusqlite_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            market_code: row.get("market_code")?,
            next_attempt_at: row.get("next_attempt_at")?,
            scheduled_at: row.get("scheduled_at")?,
            created_at: row.get("created_at")?,
        })
    }
}

#[derive(Debug, Clone)]
struct CreateTaskInput {
    market_code: Option<String>,
    next_attempt_at: Option<i64>,
    scheduled_at: Option<i64>,
    created_at: i64,
}

impl CreateModelInput<Task> for CreateTaskInput {
    fn sql_values(&self) -> Vec<SqlColumnValue> {
        vec![
            SqlColumnValue {
                column: "market_code",
                value: match &self.market_code {
                    Some(s) => SqlValue::String(s.clone()),
                    None => SqlValue::NullString,
                },
            },
            SqlColumnValue {
                column: "next_attempt_at",
                value: match self.next_attempt_at {
                    Some(v) => SqlValue::Int(v),
                    None => SqlValue::NullInt,
                },
            },
            SqlColumnValue {
                column: "scheduled_at",
                value: match self.scheduled_at {
                    Some(v) => SqlValue::Int(v),
                    None => SqlValue::NullInt,
                },
            },
            SqlColumnValue {
                column: "created_at",
                value: SqlValue::Int(self.created_at),
            },
        ]
    }
}

#[derive(Debug, Clone, Default)]
struct UpdateTaskInput;
impl UpdateModelInput<Task> for UpdateTaskInput {
    fn sql_values(&self) -> Vec<SqlColumnValue> {
        Vec::new()
    }
}

const COLUMNS: &[ModelColumn] = &[
    ModelColumn { rust_name: "id", sql_name: "id" },
    ModelColumn { rust_name: "market_code", sql_name: "market_code" },
    ModelColumn { rust_name: "next_attempt_at", sql_name: "next_attempt_at" },
    ModelColumn { rust_name: "scheduled_at", sql_name: "scheduled_at" },
    ModelColumn { rust_name: "created_at", sql_name: "created_at" },
];

static TASK_DESCRIPTOR: ModelDescriptor<Task, i64> = ModelDescriptor::new(
    "Task", "tasks", COLUMNS, "id",
    &[], &[], &[], &[], &[], &[], &[], &[], &[], &[], &[], &[], &[], &[], &[],
    None, false, &[], &[], None, None, &[],
);

fn setup() -> RusqliteRuntime {
    let runtime = RusqliteRuntime::open_in_memory().expect("open in-memory sqlite");
    runtime
        .with_connection(|conn| {
            conn.execute_batch(
                "CREATE TABLE tasks (
                    id INTEGER PRIMARY KEY,
                    market_code TEXT,
                    next_attempt_at INTEGER,
                    scheduled_at INTEGER,
                    created_at INTEGER NOT NULL
                )",
            )
            .expect("apply DDL");
            Ok(())
        })
        .unwrap();
    runtime
}

fn seed_default(delegate: &ModelDelegate<'_, Task, i64>) -> Vec<Task> {
    let inputs = vec![
        CreateTaskInput {
            market_code: Some("us".into()),
            next_attempt_at: Some(100),
            scheduled_at: None,
            created_at: 1,
        },
        CreateTaskInput {
            market_code: Some("eu".into()),
            next_attempt_at: None,
            scheduled_at: Some(200),
            created_at: 2,
        },
        CreateTaskInput {
            market_code: None,
            next_attempt_at: None,
            scheduled_at: None,
            created_at: 5,
        },
    ];
    inputs
        .into_iter()
        .map(|input| delegate.create(input).run().unwrap())
        .collect()
}

// ───── #7 eq_or_null + match_optional + where_optional ───────────────────────

#[test]
fn eq_or_null_matches_value_and_nulls() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &TASK_DESCRIPTOR);
    let _ = seed_default(&delegate);

    let market = FieldRef::<Task, String>::new("market_code");
    // Asking for "us" should match both the us row and the null-
    // market row, but NOT the eu row.
    let hits: Vec<Task> = delegate
        .find_many()
        .where_(market.eq_or_null("us"))
        .run()
        .expect("query succeeds");
    let codes: Vec<Option<String>> = hits.iter().map(|t| t.market_code.clone()).collect();
    assert_eq!(codes, vec![Some("us".into()), None]);
}

#[test]
fn match_optional_some_filters_to_value_and_nulls() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &TASK_DESCRIPTOR);
    let _ = seed_default(&delegate);

    let market = FieldRef::<Task, String>::new("market_code");
    let user_input: Option<&str> = Some("eu");
    let hits: Vec<Task> = delegate
        .find_many()
        .where_optional(market.match_optional(user_input))
        .run()
        .unwrap();
    assert_eq!(hits.len(), 2, "matches eu row + null-market row");
}

#[test]
fn match_optional_none_returns_all_rows() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &TASK_DESCRIPTOR);
    let _ = seed_default(&delegate);

    let market = FieldRef::<Task, String>::new("market_code");
    let user_input: Option<&str> = None;
    let hits: Vec<Task> = delegate
        .find_many()
        .where_optional(market.match_optional(user_input))
        .run()
        .unwrap();
    assert_eq!(hits.len(), 3, "no filter applied → all rows returned");
}

// ───── #13 coalesce ──────────────────────────────────────────────────────────

#[test]
fn coalesce_lte_uses_first_non_null_value() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &TASK_DESCRIPTOR);
    let _ = seed_default(&delegate);

    // COALESCE(next_attempt_at, scheduled_at, created_at) <= 100:
    //   row 1: COALESCE(100, NULL, 1) = 100 → 100 <= 100 ✓
    //   row 2: COALESCE(NULL, 200, 2) = 200 → 200 <= 100 ✗
    //   row 3: COALESCE(NULL, NULL, 5) = 5 → 5 <= 100 ✓
    let hits: Vec<Task> = delegate
        .find_many()
        .where_expr(
            coalesce(["next_attempt_at", "scheduled_at", "created_at"]).lte(100_i64),
        )
        .run()
        .unwrap();
    let ids: Vec<i64> = hits.iter().map(|t| t.id).collect();
    assert_eq!(ids.len(), 2, "expected 2 hits, got rows: {ids:?}");
}

#[test]
fn coalesce_accepts_fieldref_inputs() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &TASK_DESCRIPTOR);
    let _ = seed_default(&delegate);

    let scheduled = FieldRef::<Task, Option<i64>>::new("scheduled_at");
    let created = FieldRef::<Task, i64>::new("created_at");
    // Only row 2 has a scheduled_at; rows 1 and 3 fall back to created_at.
    let hits: Vec<Task> = delegate
        .find_many()
        .where_expr(coalesce([scheduled.column_name(), created.column_name()]).gte(5_i64))
        .run()
        .unwrap();
    assert_eq!(hits.len(), 2, "rows 2 (scheduled=200) and 3 (created=5)");
}

#[test]
fn coalesce_is_null_matches_only_rows_where_every_column_is_null() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &TASK_DESCRIPTOR);
    let _ = seed_default(&delegate);

    // No row has all three time columns null (created_at is NOT NULL),
    // so coalesce(...).is_null() returns nothing.
    let hits: Vec<Task> = delegate
        .find_many()
        .where_expr(coalesce(["next_attempt_at", "scheduled_at", "created_at"]).is_null())
        .run()
        .unwrap();
    assert!(hits.is_empty());
}
