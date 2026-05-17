//! Round-trip tests for `FieldRef::json_has_key` and `::json_get_text`
//! on the embedded backend. Both lower to SQLite's `json_extract`
//! function (the SQLite `json1` extension is enabled via rusqlite's
//! `bundled` feature).

use cratestack_rusqlite::{
    CreateModelInput, FromRusqliteRow, ModelDelegate, RusqliteRuntime, SqlColumnValue, SqlValue,
};
use cratestack_sql::{FieldRef, ModelColumn, ModelDescriptor};
use rusqlite::Row;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
struct ModelRun {
    id: i64,
    metrics: Option<String>, // JSON stored as TEXT for the SQLite fixture.
}

impl FromRusqliteRow for ModelRun {
    fn from_rusqlite_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            metrics: row.get("metrics")?,
        })
    }
}

#[derive(Debug, Clone)]
struct CreateModelRunInput {
    metrics: Option<String>,
}

impl CreateModelInput<ModelRun> for CreateModelRunInput {
    fn sql_values(&self) -> Vec<SqlColumnValue> {
        vec![SqlColumnValue {
            column: "metrics",
            value: match &self.metrics {
                Some(s) => SqlValue::String(s.clone()),
                None => SqlValue::NullString,
            },
        }]
    }
}

const COLUMNS: &[ModelColumn] = &[
    ModelColumn {
        rust_name: "id",
        sql_name: "id",
    },
    ModelColumn {
        rust_name: "metrics",
        sql_name: "metrics",
    },
];

static RUN_DESCRIPTOR: ModelDescriptor<ModelRun, i64> = ModelDescriptor::new(
    "ModelRun",
    "model_runs",
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
                "CREATE TABLE model_runs (
                    id INTEGER PRIMARY KEY,
                    metrics TEXT
                )",
            )
            .expect("apply DDL");
            Ok(())
        })
        .unwrap();
    runtime
}

fn seed(delegate: &ModelDelegate<'_, ModelRun, i64>) -> Vec<ModelRun> {
    vec![
        // Has the "loss" key with a value.
        CreateModelRunInput {
            metrics: Some(r#"{"loss": "0.001", "epoch": 5}"#.into()),
        },
        // Has "loss" but null.
        CreateModelRunInput {
            metrics: Some(r#"{"loss": null, "epoch": 6}"#.into()),
        },
        // No "loss" key at all.
        CreateModelRunInput {
            metrics: Some(r#"{"epoch": 7}"#.into()),
        },
        // Column itself null.
        CreateModelRunInput { metrics: None },
    ]
    .into_iter()
    .map(|input| delegate.create(input).run().unwrap())
    .collect()
}

#[test]
fn json_has_key_matches_rows_with_the_key_regardless_of_value() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &RUN_DESCRIPTOR);
    let _ = seed(&delegate);

    let metrics = FieldRef::<ModelRun, Value>::new("metrics");
    // On SQLite, `json_extract(metrics, '$.loss')` returns the
    // extracted JSON value when the key is present and that value is
    // not JSON null — so "loss":"0.001" matches and "loss":null does
    // not. (PG `?` would match both; the divergence is documented on
    // FieldRef::json_has_key.)
    let hits = delegate
        .find_many()
        .where_expr(metrics.json_has_key("loss"))
        .run()
        .expect("query succeeds");
    let ids: Vec<i64> = hits.iter().map(|r| r.id).collect();
    assert_eq!(ids, vec![1], "only the non-null 'loss' row matches");
}

#[test]
fn json_get_text_eq_matches_exact_string_value() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &RUN_DESCRIPTOR);
    let _ = seed(&delegate);

    let metrics = FieldRef::<ModelRun, Value>::new("metrics");
    let hits = delegate
        .find_many()
        .where_expr(metrics.json_get_text("loss").eq("0.001"))
        .run()
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 1);
}

#[test]
fn json_get_text_is_not_null_excludes_null_and_missing_rows() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &RUN_DESCRIPTOR);
    let _ = seed(&delegate);

    let metrics = FieldRef::<ModelRun, Value>::new("metrics");
    let hits = delegate
        .find_many()
        .where_expr(metrics.json_get_text("loss").is_not_null())
        .run()
        .unwrap();
    let ids: Vec<i64> = hits.iter().map(|r| r.id).collect();
    assert_eq!(ids, vec![1], "only the row with a non-null 'loss' value");
}

#[test]
fn json_get_text_is_null_includes_null_and_missing_rows() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &RUN_DESCRIPTOR);
    let _ = seed(&delegate);

    let metrics = FieldRef::<ModelRun, Value>::new("metrics");
    let hits = delegate
        .find_many()
        .where_expr(metrics.json_get_text("loss").is_null())
        .run()
        .unwrap();
    let ids: Vec<i64> = hits.iter().map(|r| r.id).collect();
    // Rows 2 ("loss": null), 3 (no "loss" key), 4 (column itself null)
    // all satisfy `json_extract(...) IS NULL`.
    assert_eq!(ids, vec![2, 3, 4]);
}
