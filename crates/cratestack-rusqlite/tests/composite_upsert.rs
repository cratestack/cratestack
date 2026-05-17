//! Round-trip tests for `.on_conflict(ConflictTarget::Columns(...))` on
//! the embedded backend.
//!
//! Uses a hand-written `Slot` model whose PK is `id` (auto-rowid via
//! `INTEGER PRIMARY KEY`) but with an additional `UNIQUE(envelope_id,
//! slot)` constraint — exercising the composite-key upsert path the
//! codegen's PK-only `.upsert()` previously couldn't reach.

use cratestack_rusqlite::{
    ConflictTarget, CreateModelInput, FromRusqliteRow, ModelDelegate, RusqliteRuntime,
    SqlColumnValue, SqlValue, UpsertModelInput,
};
use cratestack_sql::{ModelColumn, ModelDescriptor};
use rusqlite::Row;

#[derive(Debug, Clone, PartialEq)]
struct Slot {
    id: i64,
    envelope_id: String,
    slot: i64,
    payload: String,
}

impl FromRusqliteRow for Slot {
    fn from_rusqlite_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            envelope_id: row.get("envelope_id")?,
            slot: row.get("slot")?,
            payload: row.get("payload")?,
        })
    }
}

#[derive(Debug, Clone)]
struct UpsertSlotInput {
    envelope_id: String,
    slot: i64,
    payload: String,
}

impl CreateModelInput<Slot> for UpsertSlotInput {
    fn sql_values(&self) -> Vec<SqlColumnValue> {
        vec![
            SqlColumnValue {
                column: "envelope_id",
                value: SqlValue::String(self.envelope_id.clone()),
            },
            SqlColumnValue {
                column: "slot",
                value: SqlValue::Int(self.slot),
            },
            SqlColumnValue {
                column: "payload",
                value: SqlValue::String(self.payload.clone()),
            },
        ]
    }
}

impl UpsertModelInput<Slot> for UpsertSlotInput {
    fn sql_values(&self) -> Vec<SqlColumnValue> {
        <Self as CreateModelInput<Slot>>::sql_values(self)
    }

    fn primary_key_value(&self) -> SqlValue {
        // PK is server-generated; the composite-key upsert path doesn't
        // consult this value (it builds its conflict probe from the
        // named columns instead). Returning a sentinel keeps the trait
        // satisfied without polluting the runtime contract.
        SqlValue::Int(0)
    }
}

const COLUMNS: &[ModelColumn] = &[
    ModelColumn {
        rust_name: "id",
        sql_name: "id",
    },
    ModelColumn {
        rust_name: "envelope_id",
        sql_name: "envelope_id",
    },
    ModelColumn {
        rust_name: "slot",
        sql_name: "slot",
    },
    ModelColumn {
        rust_name: "payload",
        sql_name: "payload",
    },
];

// `upsert_update_columns` lists the non-key fields the DO UPDATE clause
// should overwrite. For a composite-key upsert the conflict tuple
// columns are NOT in this list (the engine rejects assigning to a
// column being matched on).
static UPSERT_UPDATE_COLUMNS: &[&str] = &["payload"];

static SLOT_DESCRIPTOR: ModelDescriptor<Slot, i64> = ModelDescriptor::new(
    "Slot",
    "slots",
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
    UPSERT_UPDATE_COLUMNS,
);

fn setup() -> RusqliteRuntime {
    let runtime = RusqliteRuntime::open_in_memory().expect("open in-memory sqlite");
    runtime
        .with_connection(|conn| {
            conn.execute_batch(
                "CREATE TABLE slots (
                    id INTEGER PRIMARY KEY,
                    envelope_id TEXT NOT NULL,
                    slot INTEGER NOT NULL,
                    payload TEXT NOT NULL,
                    UNIQUE(envelope_id, slot)
                )",
            )
            .expect("apply DDL");
            Ok(())
        })
        .unwrap();
    runtime
}

#[test]
fn composite_key_upsert_inserts_then_updates_payload_on_second_call() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &SLOT_DESCRIPTOR);

    let first = delegate
        .upsert(UpsertSlotInput {
            envelope_id: "env-A".into(),
            slot: 1,
            payload: "v1".into(),
        })
        .on_conflict(ConflictTarget::Columns(&["envelope_id", "slot"]))
        .run()
        .expect("first upsert inserts");
    assert_eq!(first.payload, "v1");
    assert!(first.id > 0);

    let second = delegate
        .upsert(UpsertSlotInput {
            envelope_id: "env-A".into(),
            slot: 1,
            payload: "v2".into(),
        })
        .on_conflict(ConflictTarget::Columns(&["envelope_id", "slot"]))
        .run()
        .expect("second upsert updates");
    assert_eq!(second.payload, "v2");
    assert_eq!(second.id, first.id, "same row, same PK");
}

#[test]
fn distinct_composite_keys_create_distinct_rows() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &SLOT_DESCRIPTOR);
    let target = ConflictTarget::Columns(&["envelope_id", "slot"]);

    let a = delegate
        .upsert(UpsertSlotInput {
            envelope_id: "env-A".into(),
            slot: 1,
            payload: "first".into(),
        })
        .on_conflict(target)
        .run()
        .unwrap();
    let b = delegate
        .upsert(UpsertSlotInput {
            envelope_id: "env-A".into(),
            slot: 2,
            payload: "second".into(),
        })
        .on_conflict(target)
        .run()
        .unwrap();
    let c = delegate
        .upsert(UpsertSlotInput {
            envelope_id: "env-B".into(),
            slot: 1,
            payload: "third".into(),
        })
        .on_conflict(target)
        .run()
        .unwrap();

    assert_ne!(a.id, b.id);
    assert_ne!(a.id, c.id);
    assert_ne!(b.id, c.id);
}

#[test]
fn pk_default_target_still_uses_primary_key() {
    // Smoke test: with no `.on_conflict(...)`, the upsert still targets
    // the PK — verifies the default hasn't shifted.
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &SLOT_DESCRIPTOR);

    let preview = delegate
        .upsert(UpsertSlotInput {
            envelope_id: "env-A".into(),
            slot: 1,
            payload: "x".into(),
        })
        .preview_sql();
    assert!(
        preview.contains("ON CONFLICT (id) DO UPDATE SET"),
        "got: {preview}",
    );
}
