//! Round-trip tests for `FindMany::include(...)` — to-one relation
//! side-loading on the embedded backend. Two hand-written model
//! fixtures (`Subscription` parent + `Delivery` child carrying a
//! nullable FK) exercise the matched / unmatched / null-FK shapes.

use cratestack_rusqlite::{
    CreateModelInput, FromRusqliteRow, ModelDelegate, ModelPrimaryKey, RelationInclude,
    RusqliteRuntime, SqlColumnValue, SqlValue,
};
use cratestack_sql::{FieldRef, ModelColumn, ModelDescriptor};
use rusqlite::Row;

// ───── Subscription (related side) ───────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
struct Subscription {
    id: i64,
    label: String,
}

impl FromRusqliteRow for Subscription {
    fn from_rusqlite_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            label: row.get("label")?,
        })
    }
}

impl ModelPrimaryKey<i64> for Subscription {
    fn primary_key(&self) -> i64 {
        self.id
    }
}

#[derive(Debug, Clone)]
struct CreateSubscriptionInput {
    label: String,
}

impl CreateModelInput<Subscription> for CreateSubscriptionInput {
    fn sql_values(&self) -> Vec<SqlColumnValue> {
        vec![SqlColumnValue {
            column: "label",
            value: SqlValue::String(self.label.clone()),
        }]
    }
}

const SUBSCRIPTION_COLUMNS: &[ModelColumn] = &[
    ModelColumn {
        rust_name: "id",
        sql_name: "id",
    },
    ModelColumn {
        rust_name: "label",
        sql_name: "label",
    },
];

static SUBSCRIPTION_DESCRIPTOR: ModelDescriptor<Subscription, i64> = ModelDescriptor::new(
    "Subscription",
    "subscriptions",
    SUBSCRIPTION_COLUMNS,
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

// ───── Delivery (parent side, carries an optional FK) ───────────────────────

#[derive(Debug, Clone, PartialEq)]
struct Delivery {
    id: i64,
    subscription_id: Option<i64>,
    label: String,
}

impl FromRusqliteRow for Delivery {
    fn from_rusqlite_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            subscription_id: row.get("subscription_id")?,
            label: row.get("label")?,
        })
    }
}

impl ModelPrimaryKey<i64> for Delivery {
    fn primary_key(&self) -> i64 {
        self.id
    }
}

#[derive(Debug, Clone)]
struct CreateDeliveryInput {
    subscription_id: Option<i64>,
    label: String,
}

impl CreateModelInput<Delivery> for CreateDeliveryInput {
    fn sql_values(&self) -> Vec<SqlColumnValue> {
        vec![
            SqlColumnValue {
                column: "subscription_id",
                value: match self.subscription_id {
                    Some(v) => SqlValue::Int(v),
                    None => SqlValue::NullInt,
                },
            },
            SqlColumnValue {
                column: "label",
                value: SqlValue::String(self.label.clone()),
            },
        ]
    }
}

const DELIVERY_COLUMNS: &[ModelColumn] = &[
    ModelColumn {
        rust_name: "id",
        sql_name: "id",
    },
    ModelColumn {
        rust_name: "subscription_id",
        sql_name: "subscription_id",
    },
    ModelColumn {
        rust_name: "label",
        sql_name: "label",
    },
];

static DELIVERY_DESCRIPTOR: ModelDescriptor<Delivery, i64> = ModelDescriptor::new(
    "Delivery",
    "deliveries",
    DELIVERY_COLUMNS,
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
                "CREATE TABLE subscriptions (
                    id INTEGER PRIMARY KEY,
                    label TEXT NOT NULL
                );
                CREATE TABLE deliveries (
                    id INTEGER PRIMARY KEY,
                    subscription_id INTEGER,
                    label TEXT NOT NULL
                );",
            )
            .expect("apply DDL");
            Ok(())
        })
        .unwrap();
    runtime
}

fn seed(runtime: &RusqliteRuntime) {
    let sub_delegate = ModelDelegate::new(runtime, &SUBSCRIPTION_DESCRIPTOR);
    let _a = sub_delegate
        .create(CreateSubscriptionInput { label: "a".into() })
        .run()
        .unwrap();
    let _b = sub_delegate
        .create(CreateSubscriptionInput { label: "b".into() })
        .run()
        .unwrap();

    let del_delegate = ModelDelegate::new(runtime, &DELIVERY_DESCRIPTOR);
    for (sub_id, label) in &[
        (Some(1_i64), "d1-to-a"),
        (Some(1_i64), "d2-to-a"),
        (Some(2_i64), "d3-to-b"),
        (None, "d4-orphan"),           // null FK
        (Some(99_i64), "d5-dangling"), // FK references missing subscription
    ] {
        del_delegate
            .create(CreateDeliveryInput {
                subscription_id: *sub_id,
                label: (*label).into(),
            })
            .run()
            .unwrap();
    }
}

fn subscription_relation() -> RelationInclude<Delivery, Subscription, i64> {
    RelationInclude {
        parent_fk_extract: |d: &Delivery| d.subscription_id,
        related_descriptor: &SUBSCRIPTION_DESCRIPTOR,
    }
}

#[test]
fn include_resolves_matched_relations_in_one_extra_query() {
    let runtime = setup();
    seed(&runtime);
    let delegate = ModelDelegate::new(&runtime, &DELIVERY_DESCRIPTOR);

    let label = FieldRef::<Delivery, String>::new("label");
    let pairs: Vec<(Delivery, Option<Subscription>)> = delegate
        .find_many()
        .include(subscription_relation())
        .order_by(label.asc())
        .run()
        .expect("include round-trip succeeds");

    // d1 + d2 → Some(sub a), d3 → Some(sub b), d4 (null FK) → None,
    // d5 (dangling FK 99) → None.
    let summary: Vec<(String, Option<String>)> = pairs
        .iter()
        .map(|(d, s)| (d.label.clone(), s.as_ref().map(|s| s.label.clone())))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("d1-to-a".to_string(), Some("a".to_string())),
            ("d2-to-a".to_string(), Some("a".to_string())),
            ("d3-to-b".to_string(), Some("b".to_string())),
            ("d4-orphan".to_string(), None),
            ("d5-dangling".to_string(), None),
        ],
    );
}

#[test]
fn include_filters_and_paginates_apply_to_parent_only() {
    let runtime = setup();
    seed(&runtime);
    let delegate = ModelDelegate::new(&runtime, &DELIVERY_DESCRIPTOR);

    let sub_id = FieldRef::<Delivery, i64>::new("subscription_id");
    let label = FieldRef::<Delivery, String>::new("label");
    let pairs = delegate
        .find_many()
        .include(subscription_relation())
        .where_(sub_id.eq(1_i64))
        .order_by(label.asc())
        .limit(1)
        .run()
        .unwrap();

    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].0.label, "d1-to-a");
    assert_eq!(pairs[0].1.as_ref().unwrap().label, "a");
}

#[test]
fn include_with_empty_parent_match_skips_side_load_and_returns_empty() {
    let runtime = setup();
    seed(&runtime);
    let delegate = ModelDelegate::new(&runtime, &DELIVERY_DESCRIPTOR);

    let label = FieldRef::<Delivery, String>::new("label");
    let pairs = delegate
        .find_many()
        .include(subscription_relation())
        .where_(label.eq("nope"))
        .run()
        .unwrap();
    assert!(pairs.is_empty());
}
