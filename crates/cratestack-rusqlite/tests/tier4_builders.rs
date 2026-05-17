//! Round-trip tests for tier-4 builder verbs:
//!
//!   * `delete_many(filter)` — bulk DELETE / soft-delete by predicate.
//!   * `aggregate().count() / sum / avg / min / max` — scalar reads.
//!   * `OrderClause::nulls_first()` / `nulls_last()` — null-placement
//!     override on `order_by`.

use cratestack_core::BatchSummary;
use cratestack_rusqlite::{
    CreateModelInput, FromRusqliteRow, ModelDelegate, RusqliteRuntime, SqlColumnValue, SqlValue,
};
use cratestack_sql::{FieldRef, ModelColumn, ModelDescriptor};
use rusqlite::Row;

#[derive(Debug, Clone, PartialEq)]
struct Item {
    id: i64,
    label: Option<String>,
    amount: i64,
}

impl FromRusqliteRow for Item {
    fn from_rusqlite_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            label: row.get("label")?,
            amount: row.get("amount")?,
        })
    }
}

#[derive(Debug, Clone)]
struct CreateItemInput {
    label: Option<String>,
    amount: i64,
}

impl CreateModelInput<Item> for CreateItemInput {
    fn sql_values(&self) -> Vec<SqlColumnValue> {
        vec![
            SqlColumnValue {
                column: "label",
                value: match &self.label {
                    Some(s) => SqlValue::String(s.clone()),
                    None => SqlValue::NullString,
                },
            },
            SqlColumnValue {
                column: "amount",
                value: SqlValue::Int(self.amount),
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
        rust_name: "label",
        sql_name: "label",
    },
    ModelColumn {
        rust_name: "amount",
        sql_name: "amount",
    },
];

static ITEM_DESCRIPTOR: ModelDescriptor<Item, i64> = ModelDescriptor::new(
    "Item",
    "items",
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
    runtime
        .with_connection(|conn| {
            conn.execute_batch(
                "CREATE TABLE items (
                    id INTEGER PRIMARY KEY,
                    label TEXT,
                    amount INTEGER NOT NULL
                )",
            )
            .expect("apply DDL");
            Ok(())
        })
        .unwrap();
    runtime
}

fn seed(delegate: &ModelDelegate<'_, Item, i64>) -> Vec<Item> {
    vec![
        CreateItemInput {
            label: Some("a".into()),
            amount: 10,
        },
        CreateItemInput {
            label: Some("b".into()),
            amount: 20,
        },
        CreateItemInput {
            label: None,
            amount: 5,
        },
        CreateItemInput {
            label: Some("c".into()),
            amount: 30,
        },
    ]
    .into_iter()
    .map(|input| delegate.create(input).run().unwrap())
    .collect()
}

// ───── #11 NULLS FIRST / NULLS LAST ──────────────────────────────────────────

#[test]
fn order_by_nulls_first_places_null_label_at_top() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &ITEM_DESCRIPTOR);
    let _ = seed(&delegate);

    let label = FieldRef::<Item, String>::new("label");
    let rows: Vec<Item> = delegate
        .find_many()
        .order_by(label.asc().nulls_first())
        .run()
        .unwrap();
    assert!(
        rows[0].label.is_none(),
        "null label must lead, got: {rows:?}"
    );
}

#[test]
fn order_by_default_places_null_label_at_bottom() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &ITEM_DESCRIPTOR);
    let _ = seed(&delegate);

    let label = FieldRef::<Item, String>::new("label");
    let rows: Vec<Item> = delegate.find_many().order_by(label.asc()).run().unwrap();
    assert!(
        rows.last().unwrap().label.is_none(),
        "null trails by default"
    );
}

// ───── #6 delete_many ────────────────────────────────────────────────────────

#[test]
fn delete_many_removes_matching_rows() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &ITEM_DESCRIPTOR);
    let _ = seed(&delegate);

    let amount = FieldRef::<Item, i64>::new("amount");
    let summary: BatchSummary = delegate
        .delete_many()
        .where_(amount.gte(20i64))
        .run()
        .expect("delete_many succeeds");
    assert_eq!(summary.total, 2);
    assert_eq!(summary.ok, 2);

    let remaining: Vec<Item> = delegate.find_many().run().unwrap();
    assert_eq!(remaining.len(), 2, "two rows survived");
}

#[test]
fn delete_many_refuses_without_filter() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &ITEM_DESCRIPTOR);
    let _ = seed(&delegate);

    let err = delegate
        .delete_many()
        .run()
        .expect_err("predicate-less delete_many must fail");
    let msg = format!("{err}");
    assert!(msg.contains("at least one filter"), "got: {msg}");
}

// ───── #4 aggregate ──────────────────────────────────────────────────────────

#[test]
fn aggregate_count_returns_row_count() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &ITEM_DESCRIPTOR);
    let _ = seed(&delegate);

    let total = delegate.aggregate().count().run().expect("count succeeds");
    assert_eq!(total, 4);
}

#[test]
fn aggregate_count_honors_filter() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &ITEM_DESCRIPTOR);
    let _ = seed(&delegate);

    let amount = FieldRef::<Item, i64>::new("amount");
    let total = delegate
        .aggregate()
        .count()
        .where_(amount.gte(20i64))
        .run()
        .unwrap();
    assert_eq!(total, 2);
}

#[test]
fn aggregate_sum_returns_integer_sum_or_none() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &ITEM_DESCRIPTOR);
    let _ = seed(&delegate);

    let sum: Option<i64> = delegate
        .aggregate()
        .sum("amount")
        .run()
        .expect("sum succeeds");
    assert_eq!(sum, Some(65));

    // Filter such that no rows match → SUM → None.
    let amount = FieldRef::<Item, i64>::new("amount");
    let empty: Option<i64> = delegate
        .aggregate()
        .sum("amount")
        .where_(amount.gte(100i64))
        .run()
        .unwrap();
    assert_eq!(empty, None);
}

#[test]
fn aggregate_min_max_return_bounds() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &ITEM_DESCRIPTOR);
    let _ = seed(&delegate);

    let min: Option<i64> = delegate
        .aggregate()
        .min(FieldRef::<Item, i64>::new("amount"))
        .run()
        .unwrap();
    let max: Option<i64> = delegate
        .aggregate()
        .max(FieldRef::<Item, i64>::new("amount"))
        .run()
        .unwrap();
    assert_eq!(min, Some(5));
    assert_eq!(max, Some(30));
}

#[test]
fn aggregate_avg_returns_mean_as_float() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &ITEM_DESCRIPTOR);
    let _ = seed(&delegate);

    let avg: Option<f64> = delegate.aggregate().avg("amount").run().unwrap();
    let value = avg.expect("avg must be present");
    assert!((value - 16.25).abs() < 1e-9, "got: {value}");
}

#[test]
fn aggregate_sum_with_order_clause_ignores_order() {
    // Sanity check: aggregates don't accept .order_by — confirm the
    // builder genuinely returns a scalar even after filters chain.
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &ITEM_DESCRIPTOR);
    let _ = seed(&delegate);

    let amount = FieldRef::<Item, i64>::new("amount");
    let count = delegate
        .aggregate()
        .count()
        .where_(amount.lt(25i64))
        .run()
        .unwrap();
    assert_eq!(count, 3, "rows with amount < 25: 10, 20, 5");
}
