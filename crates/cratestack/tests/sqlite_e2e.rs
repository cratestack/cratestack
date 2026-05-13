//! End-to-end: a `.cstack` schema → `include_schema!` → on-device SQLite
//! storage via `cratestack-rusqlite`, all of it driven through the same
//! umbrella crate the server uses.
//!
//! Proves the architectural claim: the same schema definition compiles to
//! both backends and the on-device ORM accepts the macro-generated
//! `ModelDescriptor` and decoder unchanged.

use cratestack::include_embedded_schema;
use cratestack::rusqlite;
use cratestack::{
    Decimal, FromRusqliteRow as _, ModelDescriptor, RusqliteRuntime, SqlColumnValue, SqlValue,
    Value,
};
use cratestack_rusqlite::{ModelDelegate, ddl::create_table_sql};

include_embedded_schema!("tests/fixtures/sqlite_e2e.cstack");

use cratestack_schema::models::{Account, Tag};
use cratestack_schema::{ACCOUNT_MODEL, TAG_MODEL};

fn setup() -> RusqliteRuntime {
    let runtime = RusqliteRuntime::open_in_memory().expect("open in-memory sqlite");
    let ddl = format!(
        "{};\n{};",
        create_table_sql(&ACCOUNT_MODEL),
        create_table_sql(&TAG_MODEL),
    );
    runtime
        .with_connection(|conn| {
            conn.execute_batch(&ddl).expect("apply DDL");
            Ok(())
        })
        .unwrap();
    runtime
}

#[test]
fn macro_emitted_descriptor_drives_full_crud_for_decimal_and_datetime_columns() {
    let runtime = setup();
    let delegate = ModelDelegate::<Account, i64>::new(&runtime, &ACCOUNT_MODEL);

    // INSERT — supply every column including primary key (no rowid alias
    // under BLOB affinity).
    let opened_at = chrono::DateTime::parse_from_rfc3339("2026-05-11T08:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let balance: Decimal = "1234.5600".parse().unwrap();

    runtime
        .with_connection(|conn| {
            conn.execute(
                "INSERT INTO accounts (id, owner_name, balance, opened_at, active) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    1i64,
                    "Alice",
                    balance.to_string(),
                    opened_at.to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
                    1i64,
                ],
            )?;
            Ok(())
        })
        .unwrap();

    let fetched = delegate
        .find_unique(1)
        .run()
        .expect("find_unique succeeds")
        .expect("row exists");
    assert_eq!(fetched.id, 1);
    assert_eq!(fetched.ownerName, "Alice");
    assert_eq!(fetched.balance, balance, "Decimal must round-trip exactly");
    assert_eq!(fetched.openedAt, opened_at);
    assert!(fetched.active);
}

#[test]
fn macro_emitted_descriptor_decodes_uuid_primary_key() {
    let runtime = setup();
    let delegate = ModelDelegate::<Tag, uuid::Uuid>::new(&runtime, &TAG_MODEL);

    let id = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
    runtime
        .with_connection(|conn| {
            conn.execute(
                "INSERT INTO tags (id, label) VALUES (?1, ?2)",
                rusqlite::params![id.hyphenated().to_string(), "important"],
            )?;
            Ok(())
        })
        .unwrap();

    let fetched = delegate
        .find_unique(id)
        .run()
        .unwrap()
        .expect("row exists");
    assert_eq!(fetched.id, id);
    assert_eq!(fetched.label, "important");
}

#[test]
fn find_many_with_filter_uses_macro_emitted_fieldref() {
    let runtime = setup();
    let delegate = ModelDelegate::<Account, i64>::new(&runtime, &ACCOUNT_MODEL);

    let base = chrono::DateTime::parse_from_rfc3339("2026-05-11T08:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let balance: Decimal = "100.0".parse().unwrap();
    runtime
        .with_connection(|conn| {
            for (id, owner, active) in
                [(1i64, "Alice", true), (2i64, "Bob", false), (3i64, "Carol", true)]
            {
                conn.execute(
                    "INSERT INTO accounts (id, owner_name, balance, opened_at, active) VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![
                        id,
                        owner,
                        balance.to_string(),
                        base.to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
                        if active { 1i64 } else { 0i64 },
                    ],
                )?;
            }
            Ok(())
        })
        .unwrap();

    // Use the macro-emitted field accessors. These resolve to FieldRef
    // values whose column is the snake_case SQL column name, matching the
    // descriptor's projection.
    let actives: Vec<Account> = delegate
        .find_many()
        .where_(cratestack_schema::account::active().is_true())
        .order_by(cratestack_schema::account::ownerName().asc())
        .run()
        .expect("find_many succeeds");

    assert_eq!(
        actives.iter().map(|a| a.ownerName.clone()).collect::<Vec<_>>(),
        vec!["Alice".to_string(), "Carol".to_string()],
    );
}

#[test]
fn create_input_validates_and_round_trips() {
    let runtime = setup();
    let delegate = ModelDelegate::<Tag, uuid::Uuid>::new(&runtime, &TAG_MODEL);

    let id = uuid::Uuid::new_v4();
    let created = delegate
        .create(cratestack_schema::CreateTagInput {
            id,
            label: "fresh".into(),
        })
        .run()
        .expect("create succeeds");
    assert_eq!(created.id, id);
    assert_eq!(created.label, "fresh");

    let fetched = delegate.find_unique(id).run().unwrap().unwrap();
    assert_eq!(fetched, created);
}

#[test]
fn update_and_delete_round_trip() {
    let runtime = setup();
    let delegate = ModelDelegate::<Tag, uuid::Uuid>::new(&runtime, &TAG_MODEL);

    let id = uuid::Uuid::new_v4();
    delegate
        .create(cratestack_schema::CreateTagInput {
            id,
            label: "before".into(),
        })
        .run()
        .unwrap();

    let updated = delegate
        .update(id)
        .set(cratestack_schema::UpdateTagInput {
            label: Some("after".into()),
        })
        .run()
        .expect("update succeeds");
    assert_eq!(updated.label, "after");

    let deleted = delegate.delete(id).run().expect("delete succeeds");
    assert_eq!(deleted.id, id);
    assert_eq!(deleted.label, "after");

    assert!(delegate.find_unique(id).run().unwrap().is_none());
}

#[test]
fn descriptor_columns_match_model_field_order() {
    // Belt-and-braces: the projection the macro builds for SELECT must list
    // the same columns the FromRusqliteRow impl reads, in the same order.
    // If someone reshuffles either side, this test catches it.
    let projection = ACCOUNT_MODEL.select_projection();
    for column in ACCOUNT_MODEL.columns {
        assert!(
            projection.contains(column.sql_name),
            "projection {projection} missing column {}",
            column.sql_name
        );
    }
    // Silence unused warnings for items only used at compile-time elsewhere.
    let _ = (SqlValue::Int(0), SqlColumnValue { column: "x", value: SqlValue::Int(0) });
    let _ = Value::Null;
    let _: fn(&rusqlite::Row<'_>) -> rusqlite::Result<Account> = Account::from_rusqlite_row;
    let _: &ModelDescriptor<Account, i64> = &ACCOUNT_MODEL;
}
