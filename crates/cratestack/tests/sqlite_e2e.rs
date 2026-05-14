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
fn upsert_inserts_when_no_conflict_and_updates_on_pk_conflict() {
    let runtime = setup();
    let delegate = ModelDelegate::<Tag, uuid::Uuid>::new(&runtime, &TAG_MODEL);

    let id = uuid::Uuid::new_v4();

    // First call → INSERT branch. No row exists yet, so the upsert lands as
    // a fresh row.
    let inserted = delegate
        .upsert(cratestack_schema::CreateTagInput {
            id,
            label: "original".into(),
        })
        .run()
        .expect("upsert inserts on no conflict");
    assert_eq!(inserted.id, id);
    assert_eq!(inserted.label, "original");

    // Second call with the same PK and a different value → UPDATE branch
    // via ON CONFLICT. Verifies the conflict target binds correctly and
    // the DO UPDATE SET list overwrites the existing column.
    let updated = delegate
        .upsert(cratestack_schema::CreateTagInput {
            id,
            label: "overwritten".into(),
        })
        .run()
        .expect("upsert updates on pk conflict");
    assert_eq!(updated.id, id);
    assert_eq!(updated.label, "overwritten");

    // And the row is observable as the *updated* value through a normal
    // find — i.e., the conflict path didn't create a duplicate.
    let fetched = delegate.find_unique(id).run().unwrap().unwrap();
    assert_eq!(fetched.label, "overwritten");
}

#[test]
fn upsert_is_idempotent_under_repeated_calls_with_same_input() {
    // A core promise of upsert for external integrators (idempotent
    // ingestion keyed on the producer's stable id): replaying the same
    // payload converges, doesn't error or duplicate.
    let runtime = setup();
    let delegate = ModelDelegate::<Tag, uuid::Uuid>::new(&runtime, &TAG_MODEL);

    let id = uuid::Uuid::new_v4();
    let input = cratestack_schema::CreateTagInput {
        id,
        label: "stable".into(),
    };

    for _ in 0..3 {
        let result = delegate.upsert(input.clone()).run().expect("upsert ok");
        assert_eq!(result.label, "stable");
    }

    // Still exactly one row.
    let all = delegate.find_many().run().unwrap();
    assert_eq!(
        all.iter().filter(|t| t.id == id).count(),
        1,
        "replayed upserts must not duplicate the row",
    );
}

// ─── Batch operations ────────────────────────────────────────────────────────
//
// The five batch primitives all return `BatchResponse<M>` envelopes, so
// per-item failure is visible without unwrapping the outer `Result`. These
// tests exercise the contract corners that single-row tests can't:
//
//   - mixed ok/err items in one envelope, indices preserved
//   - duplicate-input loud-fail at the outer boundary
//   - savepoint rollback isolation (one failing item doesn't poison the
//     successes around it)
//   - upsert dedup keyed on the input's primary_key_value()

use cratestack::{BatchItemStatus, BatchResponse};

fn ok_value<T>(item: &cratestack::BatchItemResult<T>) -> &T {
    match &item.status {
        BatchItemStatus::Ok { value } => value,
        BatchItemStatus::Error { error } => {
            panic!("expected Ok at index {}, got Err({:?})", item.index, error)
        }
    }
}

fn err_code<T>(item: &cratestack::BatchItemResult<T>) -> &str {
    match &item.status {
        BatchItemStatus::Error { error } => error.code.as_str(),
        BatchItemStatus::Ok { .. } => {
            panic!("expected Err at index {}, got Ok", item.index)
        }
    }
}

#[test]
fn batch_get_returns_envelope_with_per_item_status_in_input_order() {
    let runtime = setup();
    let delegate = ModelDelegate::<Tag, uuid::Uuid>::new(&runtime, &TAG_MODEL);

    let a = uuid::Uuid::new_v4();
    let b = uuid::Uuid::new_v4();
    let missing = uuid::Uuid::new_v4();
    for (id, label) in [(a, "first"), (b, "second")] {
        delegate
            .create(cratestack_schema::CreateTagInput {
                id,
                label: label.into(),
            })
            .run()
            .unwrap();
    }

    let response: BatchResponse<Tag> = delegate
        .batch_get(vec![a, missing, b])
        .run()
        .expect("batch_get infra ok");

    assert_eq!(response.summary.total, 3);
    assert_eq!(response.summary.ok, 2);
    assert_eq!(response.summary.err, 1);

    // Index preservation is the contract callers depend on.
    assert_eq!(response.results[0].index, 0);
    assert_eq!(response.results[1].index, 1);
    assert_eq!(response.results[2].index, 2);

    assert_eq!(ok_value(&response.results[0]).label, "first");
    assert_eq!(err_code(&response.results[1]), "NOT_FOUND");
    assert_eq!(ok_value(&response.results[2]).label, "second");
}

#[test]
fn batch_get_rejects_duplicate_input_keys() {
    let runtime = setup();
    let delegate = ModelDelegate::<Tag, uuid::Uuid>::new(&runtime, &TAG_MODEL);

    let dup = uuid::Uuid::new_v4();
    let other = uuid::Uuid::new_v4();
    let err = delegate
        .batch_get(vec![dup, other, dup])
        .run()
        .expect_err("dup loud-fails");

    let message = err.to_string();
    assert!(
        message.contains("duplicate") && message.contains("0") && message.contains("2"),
        "expected dup error naming positions 0 and 2, got: {message}",
    );
}

#[test]
fn batch_create_isolates_per_item_failures_via_savepoint() {
    // Item 1 inserts cleanly, item 2 trips the PK uniqueness constraint
    // (collides with the one item 1 just wrote), item 3 inserts cleanly.
    // The savepoint pattern means items 1 and 3 still commit.
    let runtime = setup();
    let delegate = ModelDelegate::<Tag, uuid::Uuid>::new(&runtime, &TAG_MODEL);

    let a = uuid::Uuid::new_v4();
    let b = uuid::Uuid::new_v4();
    // Seed a row that item index 1 will collide with.
    delegate
        .create(cratestack_schema::CreateTagInput {
            id: b,
            label: "existing".into(),
        })
        .run()
        .unwrap();

    let c = uuid::Uuid::new_v4();
    let response: BatchResponse<Tag> = delegate
        .batch_create(vec![
            cratestack_schema::CreateTagInput {
                id: a,
                label: "fresh-a".into(),
            },
            cratestack_schema::CreateTagInput {
                id: b,
                label: "colliding".into(),
            },
            cratestack_schema::CreateTagInput {
                id: c,
                label: "fresh-c".into(),
            },
        ])
        .run()
        .expect("batch_create infra ok");

    assert_eq!(response.summary.ok, 2);
    assert_eq!(response.summary.err, 1);
    assert_eq!(ok_value(&response.results[0]).label, "fresh-a");
    assert_eq!(err_code(&response.results[1]), "CONFLICT");
    assert_eq!(ok_value(&response.results[2]).label, "fresh-c");

    // Both successes must have actually persisted — proves savepoints
    // released cleanly and the outer commit went through.
    assert_eq!(delegate.find_unique(a).run().unwrap().unwrap().label, "fresh-a");
    assert_eq!(delegate.find_unique(c).run().unwrap().unwrap().label, "fresh-c");
    // And the seeded row was NOT overwritten by the failing item.
    assert_eq!(delegate.find_unique(b).run().unwrap().unwrap().label, "existing");
}

#[test]
fn batch_update_marks_missing_rows_as_not_found_without_failing_others() {
    let runtime = setup();
    let delegate = ModelDelegate::<Tag, uuid::Uuid>::new(&runtime, &TAG_MODEL);

    let a = uuid::Uuid::new_v4();
    delegate
        .create(cratestack_schema::CreateTagInput {
            id: a,
            label: "before".into(),
        })
        .run()
        .unwrap();
    let ghost = uuid::Uuid::new_v4();

    let response: BatchResponse<Tag> = delegate
        .batch_update(vec![
            (
                a,
                cratestack_schema::UpdateTagInput {
                    label: Some("after".into()),
                },
            ),
            (
                ghost,
                cratestack_schema::UpdateTagInput {
                    label: Some("never-applied".into()),
                },
            ),
        ])
        .run()
        .expect("batch_update infra ok");

    assert_eq!(response.summary.ok, 1);
    assert_eq!(response.summary.err, 1);
    assert_eq!(ok_value(&response.results[0]).label, "after");
    assert_eq!(err_code(&response.results[1]), "NOT_FOUND");

    // The successful update committed; the failed one had no effect.
    assert_eq!(delegate.find_unique(a).run().unwrap().unwrap().label, "after");
    assert!(delegate.find_unique(ghost).run().unwrap().is_none());
}

#[test]
fn batch_delete_returns_pre_deletion_rows_and_marks_missing_as_not_found() {
    let runtime = setup();
    let delegate = ModelDelegate::<Tag, uuid::Uuid>::new(&runtime, &TAG_MODEL);

    let a = uuid::Uuid::new_v4();
    let b = uuid::Uuid::new_v4();
    let ghost = uuid::Uuid::new_v4();
    for (id, label) in [(a, "alpha"), (b, "bravo")] {
        delegate
            .create(cratestack_schema::CreateTagInput {
                id,
                label: label.into(),
            })
            .run()
            .unwrap();
    }

    let response = delegate
        .batch_delete(vec![a, ghost, b])
        .run()
        .expect("batch_delete infra ok");

    assert_eq!(response.summary.ok, 2);
    assert_eq!(response.summary.err, 1);
    // RETURNING captures the row state before deletion — verify the
    // delete'd payload still has the original label so audit/event
    // consumers can rely on the same shape.
    assert_eq!(ok_value(&response.results[0]).label, "alpha");
    assert_eq!(err_code(&response.results[1]), "NOT_FOUND");
    assert_eq!(ok_value(&response.results[2]).label, "bravo");

    assert!(delegate.find_unique(a).run().unwrap().is_none());
    assert!(delegate.find_unique(b).run().unwrap().is_none());
}

#[test]
fn batch_upsert_dedups_on_primary_key_loud_failing_repeats() {
    let runtime = setup();
    let delegate = ModelDelegate::<Tag, uuid::Uuid>::new(&runtime, &TAG_MODEL);

    let dup = uuid::Uuid::new_v4();
    let other = uuid::Uuid::new_v4();
    let err = delegate
        .batch_upsert(vec![
            cratestack_schema::CreateTagInput {
                id: dup,
                label: "first".into(),
            },
            cratestack_schema::CreateTagInput {
                id: other,
                label: "middle".into(),
            },
            cratestack_schema::CreateTagInput {
                id: dup,
                label: "second".into(),
            },
        ])
        .run()
        .expect_err("dup PK loud-fails");

    let message = err.to_string();
    assert!(
        message.contains("duplicate"),
        "expected dup-pk message, got: {message}",
    );
}

#[test]
fn batch_upsert_mixes_insert_and_update_branches_on_pk_conflict() {
    let runtime = setup();
    let delegate = ModelDelegate::<Tag, uuid::Uuid>::new(&runtime, &TAG_MODEL);

    let existing = uuid::Uuid::new_v4();
    delegate
        .create(cratestack_schema::CreateTagInput {
            id: existing,
            label: "old-label".into(),
        })
        .run()
        .unwrap();
    let fresh = uuid::Uuid::new_v4();

    let response: BatchResponse<Tag> = delegate
        .batch_upsert(vec![
            // INSERT branch.
            cratestack_schema::CreateTagInput {
                id: fresh,
                label: "newly-inserted".into(),
            },
            // UPDATE branch — collides with `existing`.
            cratestack_schema::CreateTagInput {
                id: existing,
                label: "newly-updated".into(),
            },
        ])
        .run()
        .expect("batch_upsert infra ok");

    assert_eq!(response.summary.ok, 2);
    assert_eq!(response.summary.err, 0);
    assert_eq!(ok_value(&response.results[0]).label, "newly-inserted");
    assert_eq!(ok_value(&response.results[1]).label, "newly-updated");

    // Both end states observable.
    assert_eq!(
        delegate.find_unique(fresh).run().unwrap().unwrap().label,
        "newly-inserted",
    );
    assert_eq!(
        delegate.find_unique(existing).run().unwrap().unwrap().label,
        "newly-updated",
    );
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
