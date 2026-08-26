//! Unit coverage for the `ConflictTarget` predicate (cratestack#741),
//! rendered via `preview_sql()` — no live Postgres needed. See
//! `crates/cratestack-pg/tests/upsert_partial_index.rs` for the
//! database-backed test that proves the conflict *probe* (not just the
//! emitted SQL) honors the same predicate.

#![cfg(test)]

use crate::{
    ConflictTarget, ModelColumn, ModelDescriptor, SqlColumnValue, SqlValue, SqlxRuntime,
    UpsertRecord, UpsertRecordDoNothing,
};

struct Marker {
    _id: i64,
}

struct UpsertMarkerInput {
    k: String,
}

impl cratestack_sql::CreateModelInput<Marker> for UpsertMarkerInput {
    fn sql_values(&self) -> Vec<SqlColumnValue> {
        vec![SqlColumnValue {
            column: "k",
            value: SqlValue::String(self.k.clone()),
        }]
    }
}

impl cratestack_sql::UpsertModelInput<Marker> for UpsertMarkerInput {
    fn sql_values(&self) -> Vec<SqlColumnValue> {
        <Self as cratestack_sql::CreateModelInput<Marker>>::sql_values(self)
    }

    fn primary_key_value(&self) -> SqlValue {
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
];
static UPSERT_UPDATE_COLUMNS: &[&str] = &["k"];

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

fn runtime() -> SqlxRuntime {
    // No live Postgres needed: `connect_lazy` only parses the URL and
    // defers the actual connection to first use, which `preview_sql()`
    // never triggers. Built off the crate's own `sqlx` compat shim
    // (`crate::sqlx::pool::PoolOptions<Postgres>`) rather than the
    // umbrella `sqlx` crate, which this crate deliberately doesn't
    // depend on outside the `pgvector` feature — see `lib.rs`'s shim
    // doc comment.
    let pool = crate::sqlx::pool::PoolOptions::<crate::sqlx::Postgres>::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    SqlxRuntime::new(pool)
}

#[tokio::test]
async fn do_update_preview_sql_is_byte_identical_when_unpredicated() {
    let runtime = runtime();
    let record = UpsertRecord {
        runtime: &runtime,
        descriptor: &MARKER_DESCRIPTOR,
        input: UpsertMarkerInput { k: "x".into() },
        conflict_target: ConflictTarget::columns(&["k"]),
    };
    let sql = record.preview_sql();
    assert!(
        sql.contains("ON CONFLICT (k) DO UPDATE SET"),
        "unpredicated ON CONFLICT must render with no WHERE clause, got: {sql}",
    );
    assert!(!sql.contains("WHERE"), "got: {sql}");
}

#[tokio::test]
async fn do_update_preview_sql_renders_predicate() {
    let runtime = runtime();
    let record = UpsertRecord {
        runtime: &runtime,
        descriptor: &MARKER_DESCRIPTOR,
        input: UpsertMarkerInput { k: "x".into() },
        conflict_target: ConflictTarget::columns(&["k"]).where_index("status = 'active'"),
    };
    let sql = record.preview_sql();
    assert!(
        sql.contains("ON CONFLICT (k) WHERE status = 'active' DO UPDATE SET"),
        "got: {sql}",
    );
}

#[tokio::test]
async fn do_nothing_preview_sql_is_byte_identical_when_unpredicated() {
    let runtime = runtime();
    let record = UpsertRecordDoNothing {
        runtime: &runtime,
        descriptor: &MARKER_DESCRIPTOR,
        input: UpsertMarkerInput { k: "x".into() },
        conflict_target: ConflictTarget::columns(&["k"]),
    };
    let sql = record.preview_sql();
    assert!(
        sql.contains("ON CONFLICT (k) DO NOTHING"),
        "unpredicated ON CONFLICT must render with no WHERE clause, got: {sql}",
    );
    assert!(!sql.contains("WHERE"), "got: {sql}");
}

#[tokio::test]
async fn do_nothing_preview_sql_renders_predicate() {
    let runtime = runtime();
    let record = UpsertRecordDoNothing {
        runtime: &runtime,
        descriptor: &MARKER_DESCRIPTOR,
        input: UpsertMarkerInput { k: "x".into() },
        conflict_target: ConflictTarget::columns(&["k"]).where_index("k IS NOT NULL"),
    };
    let sql = record.preview_sql();
    assert!(
        sql.contains("ON CONFLICT (k) WHERE k IS NOT NULL DO NOTHING"),
        "got: {sql}",
    );
}

#[test]
fn predicate_on_primary_key_is_rejected() {
    let target = ConflictTarget::PRIMARY_KEY.where_index("status = 'active'");
    let err = target.validate().expect_err("PK + predicate must error");
    let message = err.to_string();
    assert!(
        message.contains("primary key"),
        "error should explain why, got: {message}",
    );
}

#[test]
fn unpredicated_targets_validate_cleanly() {
    ConflictTarget::PRIMARY_KEY.validate().unwrap();
    ConflictTarget::columns(&["k"]).validate().unwrap();
    ConflictTarget::columns(&["k"])
        .where_index("k IS NOT NULL")
        .validate()
        .unwrap();
}

/// `preview_sql()` deliberately does NOT call `.validate()`
/// (cratestack#741 finding 3 — see `UpsertRecord::preview_sql`'s doc
/// comment for the full reasoning). This locks that decision in: an
/// invalid PK+predicate `ConflictTarget` — one `.validate()` itself
/// rejects, per `predicate_on_primary_key_is_rejected` above — must
/// still render a preview string rather than panicking, so a
/// regression that made `preview_sql()` start calling `.validate()`
/// (breaking the "every preview_sql() is infallible" convention every
/// other builder in this codebase relies on) would be caught here.
#[tokio::test]
async fn do_update_preview_sql_does_not_validate_pk_plus_predicate() {
    let runtime = runtime();
    let bad_target = ConflictTarget::PRIMARY_KEY.where_index("status = 'active'");
    assert!(bad_target.validate().is_err(), "sanity: must be invalid");
    let record = UpsertRecord {
        runtime: &runtime,
        descriptor: &MARKER_DESCRIPTOR,
        input: UpsertMarkerInput { k: "x".into() },
        conflict_target: bad_target,
    };
    let sql = record.preview_sql();
    assert!(
        sql.contains("ON CONFLICT (id) WHERE status = 'active' DO UPDATE SET"),
        "preview must still render even though the target is invalid, got: {sql}",
    );
}
