//! Round-trip tests for `.select(...)` column projection on the
//! embedded backend.
//!
//! Hand-written `Default` and `FromPartialRusqliteRow` impls stand in
//! for what the macro emits for codegen-driven schemas. The test
//! fixture has four columns; the tests project subsets of them and
//! verify that non-selected fields receive their type's
//! `Default::default()` value while selected fields carry the real
//! values from the row.

use cratestack_rusqlite::{
    CreateModelInput, FromPartialRusqliteRow, FromRusqliteRow, ModelDelegate, RusqliteRuntime,
    SqlColumnValue, SqlValue,
};
use cratestack_sql::{FieldRef, ModelColumn, ModelDescriptor};
use rusqlite::Row;

#[derive(Debug, Clone, Default, PartialEq)]
struct PaymentIntent {
    id: i64,
    connector_id: String,
    status: String,
    amount: i64,
}

impl FromRusqliteRow for PaymentIntent {
    fn from_rusqlite_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            connector_id: row.get("connector_id")?,
            status: row.get("status")?,
            amount: row.get("amount")?,
        })
    }
}

impl FromPartialRusqliteRow for PaymentIntent {
    fn from_partial_rusqlite_row(row: &Row<'_>, selected: &[&str]) -> rusqlite::Result<Self> {
        Ok(Self {
            id: if selected.iter().any(|c| *c == "id") {
                row.get("id")?
            } else {
                Default::default()
            },
            connector_id: if selected.iter().any(|c| *c == "connector_id") {
                row.get("connector_id")?
            } else {
                Default::default()
            },
            status: if selected.iter().any(|c| *c == "status") {
                row.get("status")?
            } else {
                Default::default()
            },
            amount: if selected.iter().any(|c| *c == "amount") {
                row.get("amount")?
            } else {
                Default::default()
            },
        })
    }
}

#[derive(Debug, Clone)]
struct CreatePaymentIntentInput {
    connector_id: String,
    status: String,
    amount: i64,
}

impl CreateModelInput<PaymentIntent> for CreatePaymentIntentInput {
    fn sql_values(&self) -> Vec<SqlColumnValue> {
        vec![
            SqlColumnValue {
                column: "connector_id",
                value: SqlValue::String(self.connector_id.clone()),
            },
            SqlColumnValue {
                column: "status",
                value: SqlValue::String(self.status.clone()),
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
        rust_name: "connector_id",
        sql_name: "connector_id",
    },
    ModelColumn {
        rust_name: "status",
        sql_name: "status",
    },
    ModelColumn {
        rust_name: "amount",
        sql_name: "amount",
    },
];

static PAYMENT_INTENT_DESCRIPTOR: ModelDescriptor<PaymentIntent, i64> = ModelDescriptor::new(
    "PaymentIntent",
    "payment_intents",
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
                "CREATE TABLE payment_intents (
                    id INTEGER PRIMARY KEY,
                    connector_id TEXT NOT NULL,
                    status TEXT NOT NULL,
                    amount INTEGER NOT NULL
                )",
            )
            .expect("apply DDL");
            Ok(())
        })
        .unwrap();
    runtime
}

fn seed(delegate: &ModelDelegate<'_, PaymentIntent, i64>) -> Vec<PaymentIntent> {
    vec![
        CreatePaymentIntentInput {
            connector_id: "stripe_live".into(),
            status: "succeeded".into(),
            amount: 1000,
        },
        CreatePaymentIntentInput {
            connector_id: "adyen_test".into(),
            status: "pending".into(),
            amount: 250,
        },
    ]
    .into_iter()
    .map(|input| delegate.create(input).run().unwrap())
    .collect()
}

#[test]
fn find_unique_select_populates_only_requested_field() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &PAYMENT_INTENT_DESCRIPTOR);
    let rows = seed(&delegate);
    let target_id = rows[0].id;

    let projection = delegate
        .find_unique(target_id)
        .select(["connector_id"])
        .run()
        .unwrap()
        .expect("row exists");

    assert!(projection.is_selected("connector_id"));
    assert!(!projection.is_selected("status"));
    assert!(!projection.is_selected("amount"));

    // The selected field holds the real value; the others zero-default.
    assert_eq!(projection.value.connector_id, "stripe_live");
    assert_eq!(projection.value.status, "", "non-selected default");
    assert_eq!(projection.value.amount, 0, "non-selected default");
}

#[test]
fn find_unique_select_accepts_fieldref_inputs() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &PAYMENT_INTENT_DESCRIPTOR);
    let rows = seed(&delegate);
    let target_id = rows[1].id;

    // FieldRef → column-name extraction via IntoColumnName. Mixed
    // T params can't share an array literal, so we go through
    // `.column_name()` at the call site for heterogeneous tuples;
    // homogeneous-T cases (`[col_a, col_b]` where both are
    // `FieldRef<M, T>`) flow directly.
    let connector_ref = FieldRef::<PaymentIntent, String>::new("connector_id");
    let amount_ref = FieldRef::<PaymentIntent, i64>::new("amount");

    let projection = delegate
        .find_unique(target_id)
        .select([connector_ref.column_name(), amount_ref.column_name()])
        .run()
        .unwrap()
        .expect("row exists");

    assert_eq!(projection.value.connector_id, "adyen_test");
    assert_eq!(projection.value.amount, 250);
    assert_eq!(projection.value.status, "", "status not in selection");
}

#[test]
fn find_many_select_projects_each_row() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &PAYMENT_INTENT_DESCRIPTOR);
    let _ = seed(&delegate);

    let amount = FieldRef::<PaymentIntent, i64>::new("amount");
    let projections = delegate
        .find_many()
        .order_by(amount.asc())
        .select(["amount"])
        .run()
        .unwrap();

    assert_eq!(projections.len(), 2);
    // Smaller amount (250) first because order_by amount ASC.
    assert_eq!(projections[0].value.amount, 250);
    assert_eq!(projections[1].value.amount, 1000);
    // Non-selected fields default everywhere.
    for p in &projections {
        assert_eq!(p.value.connector_id, "");
        assert_eq!(p.value.status, "");
        assert!(!p.is_selected("connector_id"));
        assert!(p.is_selected("amount"));
    }
}

#[test]
fn find_unique_select_empty_projection_falls_back_to_primary_key() {
    // Caller passes an empty selection — the descriptor's
    // `select_projection_subset` falls back to projecting the PK so
    // the SQL is still valid. Selected slice stays empty so the
    // decoder defaults every field.
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &PAYMENT_INTENT_DESCRIPTOR);
    let rows = seed(&delegate);
    let target_id = rows[0].id;

    let projection = delegate
        .find_unique(target_id)
        .select(Vec::<&str>::new())
        .run()
        .unwrap()
        .expect("row exists");
    // Selected manifest is empty, so every field defaults.
    assert_eq!(projection.value.id, 0);
    assert_eq!(projection.value.connector_id, "");
    assert!(projection.selected.is_empty());
}
