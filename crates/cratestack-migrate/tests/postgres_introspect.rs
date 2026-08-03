#![cfg(feature = "postgres-introspect")]
//! Live-Postgres integration tests for `introspect::postgres` (issue
//! #204, design doc §5.2/§8 case pattern).
//!
//! Skips silently unless `CRATESTACK_TEST_DATABASE_URL` is set — the
//! same convention every other PG-backed test in the workspace uses
//! (`just test-pg` sets it). This crate's `postgres-introspect`
//! feature isn't part of any workspace-wide `just test-pg`/
//! `just test-pg-tc` invocation by default (it's opt-in, per the
//! ticket's "no new DB dependency for the default build" requirement),
//! so run directly:
//!
//! ```sh
//! just pg-up
//! CRATESTACK_TEST_DATABASE_URL='postgres://cratestack:cratestack@localhost:55432/cratestack_test' \
//!   cargo test -p cratestack-migrate --features postgres-introspect
//! just pg-down
//! ```
//!
//! Every table/view this file creates uses an `introspect_probe_`
//! prefix and is dropped (`IF EXISTS`) before creation, so tests are
//! safe to run repeatedly and — since each uses a distinct name — safe
//! to run concurrently against a shared database without a serializing
//! mutex.

use cratestack_migrate::introspect::postgres::introspect;
use cratestack_migrate::ir::{CheckKind, Column, ColumnArity, ColumnType};
use cratestack_migrate::{Projections, diff_projections, project};
use sqlx_core::pool::PoolOptions;
use sqlx_postgres::{PgPool, Postgres};

async fn connect_or_skip() -> Option<PgPool> {
    let url = std::env::var("CRATESTACK_TEST_DATABASE_URL").ok()?;
    PoolOptions::<Postgres>::new()
        .max_connections(2)
        .connect(&url)
        .await
        .ok()
}

async fn exec(pool: &PgPool, sql: &str) {
    sqlx_core::raw_sql::raw_sql(sql)
        .execute(pool)
        .await
        .unwrap_or_else(|error| panic!("DDL failed: {sql}\n{error}"));
}

/// Isolate a single table for comparison, ignoring whatever else might
/// exist in the (shared, possibly concurrently-used) test database.
fn only_table(projections: &Projections, table: &str) -> Projections {
    let mut out = Projections::default();
    if let Some(projection) = projections.tables.get(table) {
        out.tables.insert(table.to_owned(), projection.clone());
    }
    out
}

/// Design doc §8, case 1 (adapted to Phase B's scope): a hand-created
/// table matching a hand-authored `.cstack` schema exactly introspects
/// to the same `TableProjection` `project()` produces, and diffing the
/// two reports zero drift.
#[tokio::test]
async fn round_trip_matches_hand_authored_schema() {
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    let table = "introspection_probe_customers";

    exec(&pool, &format!("DROP TABLE IF EXISTS {table}")).await;
    exec(
        &pool,
        &format!(
            "CREATE TABLE {table} (
                id UUID NOT NULL,
                email TEXT NOT NULL,
                age BIGINT NOT NULL,
                balance DOUBLE PRECISION NOT NULL,
                verified BOOLEAN NOT NULL,
                joined_at TIMESTAMPTZ NOT NULL,
                tenant_id TEXT NOT NULL,
                region TEXT NOT NULL,
                PRIMARY KEY (id)
            );
            CREATE UNIQUE INDEX {table}_email_key ON {table} (email);
            CREATE UNIQUE INDEX {table}_tenant_id_region_key ON {table} (tenant_id, region);"
        ),
    )
    .await;

    let schema = cratestack_parser::parse_schema(
        r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model IntrospectionProbeCustomer {
  id Uuid @id
  email String @unique
  age Int
  balance Float
  verified Boolean
  joinedAt DateTime
  tenantId String
  region String

  @@unique([tenantId, region])
}
"#,
    )
    .expect("hand-authored schema should parse");
    let expected = project(&schema);

    let report = introspect(&pool)
        .await
        .expect("introspection should succeed");
    let unmapped_for_table: Vec<_> = report
        .unmapped_columns
        .iter()
        .filter(|u| u.table == table)
        .collect();
    assert!(
        unmapped_for_table.is_empty(),
        "unmapped columns: {unmapped_for_table:?}"
    );

    let introspected_table = report
        .projections
        .tables
        .get(table)
        .unwrap_or_else(|| panic!("{table} should have been introspected"));
    let expected_table = expected
        .tables
        .get(table)
        .expect("project() should produce the same table name");
    assert_eq!(introspected_table, expected_table);

    // The same claim, proven through the actual comparison engine a
    // baseline command (Phase C) would use.
    let ops = diff_projections(&only_table(&report.projections, table), &expected);
    assert!(ops.is_empty(), "expected no drift, got: {ops:?}");
}

/// A column type outside the common mapped set (`numeric`, `jsonb`,
/// arrays, …) is reported as unmapped, never guessed at — the design
/// doc's central safety rule.
#[tokio::test]
async fn unmapped_column_type_is_reported_not_guessed() {
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    let table = "introspection_probe_prices";

    exec(&pool, &format!("DROP TABLE IF EXISTS {table}")).await;
    exec(
        &pool,
        &format!(
            "CREATE TABLE {table} (
                id UUID NOT NULL PRIMARY KEY,
                amount NUMERIC(12, 2) NOT NULL,
                metadata JSONB,
                tags TEXT[]
            )"
        ),
    )
    .await;

    let report = introspect(&pool)
        .await
        .expect("introspection should succeed");
    let projected = report
        .projections
        .tables
        .get(table)
        .unwrap_or_else(|| panic!("{table} should have been introspected"));

    let mapped_names: Vec<&str> = projected.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(mapped_names, vec!["id"], "only the PK column should map");

    let mut unmapped: Vec<(&str, &str)> = report
        .unmapped_columns
        .iter()
        .filter(|u| u.table == table)
        .map(|u| (u.column.as_str(), u.postgres_type.as_str()))
        .collect();
    unmapped.sort();
    assert_eq!(
        unmapped,
        vec![
            ("amount", "numeric"),
            ("metadata", "jsonb"),
            ("tags", "_text"),
        ]
    );
}

/// A raw CHECK constraint shaped exactly like the one
/// `emit::postgres::checks` generates for a `.cstack` `enum` field
/// (`col IN (...)`, which Postgres normalises to `col = ANY (ARRAY[...])`
/// when deparsed) reconstructs to the same `CheckKind::Enum` the
/// schema-side projection would have produced.
#[tokio::test]
async fn enum_shaped_check_reconstructs_to_check_kind_enum() {
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    let table = "introspection_probe_orders";

    exec(&pool, &format!("DROP TABLE IF EXISTS {table}")).await;
    exec(
        &pool,
        &format!(
            "CREATE TABLE {table} (
                id UUID NOT NULL PRIMARY KEY,
                status TEXT NOT NULL,
                CONSTRAINT {table}_status_enum_check CHECK (status IN ('Pending', 'Shipped'))
            )"
        ),
    )
    .await;

    let report = introspect(&pool)
        .await
        .expect("introspection should succeed");
    let projected = report
        .projections
        .tables
        .get(table)
        .unwrap_or_else(|| panic!("{table} should have been introspected"));

    let check = projected
        .checks
        .iter()
        .find(|c| c.column == "status")
        .expect("status CHECK should be present");
    assert_eq!(check.name, format!("{table}_status_enum_check"));
    assert_eq!(
        check.kind,
        CheckKind::Enum {
            variants: vec!["Pending".to_owned(), "Shipped".to_owned()],
            list: false,
        }
    );
}

/// A native Postgres enum type (`CREATE TYPE ... AS ENUM`) — something
/// cratestack itself never emits, but a hand-created table might use —
/// folds into the same `CheckKind::Enum` shape, per the note carried
/// over from issue #203.
#[tokio::test]
async fn native_enum_type_folds_into_a_check() {
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    let table = "introspection_probe_tickets";
    let enum_type = "introspection_probe_ticket_status";

    exec(&pool, &format!("DROP TABLE IF EXISTS {table}")).await;
    exec(&pool, &format!("DROP TYPE IF EXISTS {enum_type}")).await;
    exec(
        &pool,
        &format!("CREATE TYPE {enum_type} AS ENUM ('Open', 'Closed')"),
    )
    .await;
    exec(
        &pool,
        &format!(
            "CREATE TABLE {table} (
                id UUID NOT NULL PRIMARY KEY,
                status {enum_type} NOT NULL
            )"
        ),
    )
    .await;

    let report = introspect(&pool)
        .await
        .expect("introspection should succeed");
    let projected = report
        .projections
        .tables
        .get(table)
        .unwrap_or_else(|| panic!("{table} should have been introspected"));

    let status = projected
        .columns
        .iter()
        .find(|c| c.name == "status")
        .expect("status column should be mapped");
    assert_eq!(
        status,
        &Column {
            name: "status".to_owned(),
            ty: ColumnType::Scalar("String".to_owned()),
            arity: ColumnArity::Required,
            default: None,
            primary_key: false,
        }
    );

    let check = projected
        .checks
        .iter()
        .find(|c| c.column == "status")
        .expect("synthetic enum CHECK should be present");
    assert_eq!(check.name, format!("{table}_status_enum_check"));
    assert_eq!(
        check.kind,
        CheckKind::Enum {
            variants: vec!["Open".to_owned(), "Closed".to_owned()],
            list: false,
        }
    );
}

#[tokio::test]
async fn view_is_introspected_with_its_source_table() {
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    let table = "introspection_probe_accounts";
    let view = "introspection_probe_active_accounts";

    exec(&pool, &format!("DROP VIEW IF EXISTS {view}")).await;
    exec(&pool, &format!("DROP TABLE IF EXISTS {table}")).await;
    exec(
        &pool,
        &format!(
            "CREATE TABLE {table} (id UUID NOT NULL PRIMARY KEY, active BOOLEAN NOT NULL);
             CREATE VIEW {view} AS SELECT id FROM {table} WHERE active = true;"
        ),
    )
    .await;

    let report = introspect(&pool)
        .await
        .expect("introspection should succeed");
    let projected = report
        .projections
        .views
        .get(view)
        .unwrap_or_else(|| panic!("{view} should have been introspected"));

    assert!(!projected.is_materialized);
    assert_eq!(projected.source_tables, vec![table.to_owned()]);
    assert!(
        projected.sql.contains(table),
        "view SQL should reference its source table: {}",
        projected.sql
    );
}

#[tokio::test]
async fn cratestack_migrations_table_itself_is_excluded() {
    let Some(pool) = connect_or_skip().await else {
        return;
    };

    exec(&pool, "DROP TABLE IF EXISTS cratestack_migrations").await;
    exec(
        &pool,
        "CREATE TABLE cratestack_migrations (
            id TEXT PRIMARY KEY,
            description TEXT NOT NULL,
            checksum BYTEA NOT NULL,
            applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )",
    )
    .await;

    let report = introspect(&pool)
        .await
        .expect("introspection should succeed");
    assert!(
        !report
            .projections
            .tables
            .contains_key("cratestack_migrations")
    );
}

/// A multi-column CHECK has no single-column `AddCheck` representation
/// in the IR — see `introspect::postgres`'s module doc "known gaps" —
/// so it's skipped rather than mis-attributed to one of its columns.
#[tokio::test]
async fn multi_column_check_is_skipped_not_mis_attributed() {
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    let table = "introspection_probe_ranges";

    exec(&pool, &format!("DROP TABLE IF EXISTS {table}")).await;
    exec(
        &pool,
        &format!(
            "CREATE TABLE {table} (
                id UUID NOT NULL PRIMARY KEY,
                low BIGINT NOT NULL,
                high BIGINT NOT NULL,
                CONSTRAINT {table}_range_check CHECK (low < high)
            )"
        ),
    )
    .await;

    let report = introspect(&pool)
        .await
        .expect("introspection should succeed");
    let projected = report
        .projections
        .tables
        .get(table)
        .unwrap_or_else(|| panic!("{table} should have been introspected"));
    assert!(
        projected.checks.is_empty(),
        "multi-column CHECK should not appear: {:?}",
        projected.checks
    );
}
