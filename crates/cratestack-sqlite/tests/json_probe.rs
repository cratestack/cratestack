//! Regression test for #231: a `Json` model field must compile through
//! `include_embedded_schema!` on the embedded/SQLite backend, and must
//! actually round-trip a JSON payload through the generated rusqlite
//! decoder — not merely compile.
//!
//! Before the fix, this file failed to compile at all:
//! `the trait bound cratestack::Json<cratestack::Value>: Default is not
//! satisfied`, because the generated partial-row decoder
//! (`crates/cratestack-macros/src/model/row_sqlite.rs`) fills non-selected
//! columns with `Default::default()`, and `cratestack_core::Json` didn't
//! derive `Default` (unlike `sqlx::types::Json` on the Postgres backend).

use std::collections::BTreeMap;

use cratestack::include_embedded_schema;
use cratestack::{RusqliteRuntime, Value};
use cratestack_rusqlite::{ModelDelegate, ddl::create_table_sql};

include_embedded_schema!("tests/fixtures/json_probe.cstack");

use cratestack_schema::PROBE_MODEL;
use cratestack_schema::models::Probe;

fn setup() -> RusqliteRuntime {
    let runtime = RusqliteRuntime::open_in_memory().expect("open in-memory sqlite");
    let ddl = create_table_sql(&PROBE_MODEL);
    runtime
        .with_connection(|conn| {
            conn.execute_batch(&ddl).expect("apply DDL");
            Ok(())
        })
        .unwrap();
    runtime
}

fn payload() -> Value {
    let mut map = BTreeMap::new();
    map.insert("currency".to_string(), Value::String("XAF".to_string()));
    map.insert("amount".to_string(), Value::Int(1_500));
    Value::Map(map)
}

#[test]
fn json_field_round_trips_through_generated_decoder() {
    let runtime = setup();
    let delegate = ModelDelegate::<Probe, i64>::new(&runtime, &PROBE_MODEL);

    let created = delegate
        .create(cratestack_schema::CreateProbeInput {
            id: 1,
            payload: cratestack::Json(payload()),
            note: Some(cratestack::Json(Value::String("hello".to_string()))),
        })
        .run()
        .expect("create succeeds");

    assert_eq!(created.id, 1);
    assert_eq!(created.payload.0, payload());
    assert_eq!(
        created.note.as_ref().map(|json| json.0.clone()),
        Some(Value::String("hello".to_string()))
    );

    let fetched = delegate
        .find_unique(1)
        .run()
        .expect("find_unique succeeds")
        .expect("row exists");
    assert_eq!(fetched.payload.0, payload());
    assert_eq!(
        fetched.note.as_ref().map(|json| json.0.clone()),
        Some(Value::String("hello".to_string()))
    );
}

#[test]
fn optional_json_field_round_trips_null() {
    let runtime = setup();
    let delegate = ModelDelegate::<Probe, i64>::new(&runtime, &PROBE_MODEL);

    let created = delegate
        .create(cratestack_schema::CreateProbeInput {
            id: 2,
            payload: cratestack::Json(payload()),
            note: None,
        })
        .run()
        .expect("create succeeds");
    assert!(created.note.is_none());

    let fetched = delegate
        .find_unique(2)
        .run()
        .expect("find_unique succeeds")
        .expect("row exists");
    assert!(fetched.note.is_none());
    assert_eq!(fetched.payload.0, payload());
}

#[test]
fn partial_select_defaults_unselected_json_field_via_default_impl() {
    // This is the exact code path the compile error blocked: the
    // partial-row decoder fills any *unselected* column with
    // `Default::default()`. Selecting only `id` forces that path for both
    // the required `payload` and optional `note` Json fields.
    let runtime = setup();
    let delegate = ModelDelegate::<Probe, i64>::new(&runtime, &PROBE_MODEL);

    delegate
        .create(cratestack_schema::CreateProbeInput {
            id: 3,
            payload: cratestack::Json(payload()),
            note: Some(cratestack::Json(Value::String("ignored".to_string()))),
        })
        .run()
        .expect("create succeeds");

    let partial = delegate
        .find_many()
        .select(["id"])
        .run()
        .expect("find_many succeeds");
    let row = &partial
        .iter()
        .find(|projection| projection.value.id == 3)
        .expect("row exists")
        .value;

    // Unselected `Json` columns must default rather than fail to compile
    // or panic at runtime.
    assert_eq!(row.payload.0, Value::default());
    assert_eq!(row.payload.0, Value::Null);
    assert_eq!(row.note, None);
}
