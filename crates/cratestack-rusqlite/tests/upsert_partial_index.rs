//! Live-SQLite round-trip tests for `ConflictTarget::where_index`
//! (cratestack#741) against a real `CREATE UNIQUE INDEX ... WHERE ...`
//! partial index — proving SQLite accepts the identical `ON CONFLICT
//! (...) WHERE <predicate>` inference form Postgres does (see
//! `render/upsert.rs`'s doc comment: confirmed against the vendored
//! libsqlite3-sys 0.37.0 / SQLite 3.51.3).
//!
//! The embedded (`cratestack-rusqlite`) upsert has no probe (no
//! `SELECT ... FOR UPDATE`, no `Inserted`/`Existing` distinction) — it
//! is a direct `INSERT ... ON CONFLICT ... DO UPDATE ... RETURNING`
//! against the live connection, so a wrong-index inference surfaces
//! immediately as a SQLite-level constraint violation or an unintended
//! row count. That is a weaker correctness hazard than the sqlx
//! backend's conflict-probe issue (see
//! `crates/cratestack-pg/tests/upsert_partial_index.rs` for that one),
//! but the SQL shape still needs to be proven against a real partial
//! index, not just asserted as a string (`render/tests_upsert.rs`
//! covers the string).

use cratestack_rusqlite::{
    ConflictTarget, CreateModelInput, FromRusqliteRow, ModelDelegate, RusqliteRuntime,
    SqlColumnValue, SqlValue, UpsertModelInput,
};
use cratestack_sql::{ModelColumn, ModelDescriptor};
use rusqlite::Row;

#[derive(Debug, Clone, PartialEq)]
struct Marker {
    id: i64,
    k: String,
    status: String,
    payload: String,
}

impl FromRusqliteRow for Marker {
    fn from_rusqlite_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            k: row.get("k")?,
            status: row.get("status")?,
            payload: row.get("payload")?,
        })
    }
}

#[derive(Debug, Clone)]
struct UpsertMarkerInput {
    k: String,
    status: String,
    payload: String,
}

impl CreateModelInput<Marker> for UpsertMarkerInput {
    fn sql_values(&self) -> Vec<SqlColumnValue> {
        vec![
            SqlColumnValue {
                column: "k",
                value: SqlValue::String(self.k.clone()),
            },
            SqlColumnValue {
                column: "status",
                value: SqlValue::String(self.status.clone()),
            },
            SqlColumnValue {
                column: "payload",
                value: SqlValue::String(self.payload.clone()),
            },
        ]
    }
}

impl UpsertModelInput<Marker> for UpsertMarkerInput {
    fn sql_values(&self) -> Vec<SqlColumnValue> {
        <Self as CreateModelInput<Marker>>::sql_values(self)
    }

    fn primary_key_value(&self) -> SqlValue {
        // Server-generated PK; the composite-key upsert path doesn't
        // consult this (same rationale as `composite_upsert.rs`).
        SqlValue::Int(0)
    }
}

const COLUMNS: &[ModelColumn] = &[
    ModelColumn {
        rust_name: "id",
        sql_name: "id",
    },
    ModelColumn {
        rust_name: "k",
        sql_name: "k",
    },
    ModelColumn {
        rust_name: "status",
        sql_name: "status",
    },
    ModelColumn {
        rust_name: "payload",
        sql_name: "payload",
    },
];

static UPSERT_UPDATE_COLUMNS: &[&str] = &["status", "payload"];

#[allow(clippy::too_many_arguments)]
static MARKER_DESCRIPTOR: ModelDescriptor<Marker, i64> = ModelDescriptor::new(
    "Marker",
    "markers",
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
                "CREATE TABLE markers (
                    id INTEGER PRIMARY KEY,
                    k TEXT,
                    status TEXT NOT NULL,
                    payload TEXT NOT NULL
                );
                CREATE UNIQUE INDEX idx_markers_active_k ON markers(k) WHERE status = 'active';",
            )
            .expect("apply DDL, including the partial unique index");
            Ok(())
        })
        .unwrap();
    runtime
}

fn input(k: &str, status: &str, payload: &str) -> UpsertMarkerInput {
    UpsertMarkerInput {
        k: k.to_owned(),
        status: status.to_owned(),
        payload: payload.to_owned(),
    }
}

// ───── the decisive non-null-test predicate case ─────────────────────────

#[test]
fn conflict_target_predicate_targets_the_partial_index_not_a_full_one() {
    // `UNIQUE (k) WHERE status = 'active'` — an `active` row with k='x'
    // exists. Upserting an `archived` row with the SAME k is outside
    // the index's uniqueness domain, so it must INSERT a second row,
    // not merge into the active one via `ON CONFLICT (k) DO UPDATE`
    // (which would happen if SQLite's index inference or this crate's
    // rendering silently fell back to a full-table conflict target).
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &MARKER_DESCRIPTOR);
    let target = ConflictTarget::columns(&["k"]).where_index("status = 'active'");

    let active = delegate
        .upsert(input("x", "active", "v1"))
        .on_conflict(target)
        .run()
        .expect("first insert (active) succeeds");

    let archived = delegate
        .upsert(input("x", "archived", "v2"))
        .on_conflict(target)
        .run()
        .expect("second insert (archived, same k) must succeed as a NEW row");

    assert_ne!(
        archived.id, active.id,
        "an archived row with the same k must be a distinct row from the active one"
    );

    let rows: Vec<Marker> = runtime
        .with_connection(|conn| {
            let mut stmt = conn
                .prepare("SELECT id, k, status, payload FROM markers ORDER BY id")
                .unwrap();
            let rows = stmt
                .query_map([], Marker::from_rusqlite_row)
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            Ok(rows)
        })
        .unwrap();
    assert_eq!(rows.len(), 2, "both rows must exist independently");
    assert_eq!(rows[0].status, "active");
    assert_eq!(rows[0].payload, "v1");
    assert_eq!(rows[1].status, "archived");
    assert_eq!(rows[1].payload, "v2");
}

#[test]
fn conflict_target_predicate_updates_within_the_partial_index() {
    // A second `active` upsert on the same k DOES conflict — it is
    // inside the partial index's domain — so it must merge via DO
    // UPDATE, same row.
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &MARKER_DESCRIPTOR);
    let target = ConflictTarget::columns(&["k"]).where_index("status = 'active'");

    let first = delegate
        .upsert(input("y", "active", "v1"))
        .on_conflict(target)
        .run()
        .expect("first insert");
    let second = delegate
        .upsert(input("y", "active", "v2"))
        .on_conflict(target)
        .run()
        .expect("second upsert merges into the same active row");

    assert_eq!(second.id, first.id);
    assert_eq!(second.payload, "v2");

    let count: i64 = runtime
        .with_connection(|conn| {
            Ok(conn
                .query_row("SELECT COUNT(*) FROM markers", [], |r| r.get(0))
                .expect("count query"))
        })
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn predicate_on_primary_key_is_rejected_before_any_sql_runs() {
    let runtime = setup();
    let delegate = ModelDelegate::new(&runtime, &MARKER_DESCRIPTOR);
    let target = ConflictTarget::PRIMARY_KEY.where_index("status = 'active'");

    let err = delegate
        .upsert(input("z", "active", "v1"))
        .on_conflict(target)
        .run()
        .expect_err("PK + predicate must be rejected, not silently dropped");
    assert!(
        err.to_string().contains("primary key"),
        "error should explain why: {err}"
    );

    let count: i64 = runtime
        .with_connection(|conn| {
            Ok(conn
                .query_row("SELECT COUNT(*) FROM markers", [], |r| r.get(0))
                .expect("count query"))
        })
        .unwrap();
    assert_eq!(count, 0, "the rejected call must not have run any SQL");
}
