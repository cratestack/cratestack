#![cfg(feature = "postgres-introspect")]
//! Live-Postgres integration tests for `introspect::postgres` (issue
//! #204, design doc §5.2/§8 case pattern).
//!
//! Backend selection mirrors the standard shim every other PG-backed test
//! in the workspace uses (`crates/cratestack-pg/tests/support/pg.rs`,
//! `crates/cratestack-outbox/tests/support/pg.rs`):
//!
//! 1. `CRATESTACK_TEST_DATABASE_URL` — connect to an external PG (the
//!    `just pg-up` / `just test-pg` flow).
//! 2. `CRATESTACK_USE_TESTCONTAINERS=1` — spawn an ephemeral PG container.
//! 3. Neither set — skip.
//!
//! `CRATESTACK_REQUIRE_DB` turns a connection/container failure into a
//! panic instead of a skip — the CI gate sets it (`just
//! test-ci-db-migrate-introspect`), so a broken Docker runner can't
//! silently green this file's tests while running none of them (a
//! 2026-08 CI-coverage audit found that is exactly what was happening:
//! this crate's `postgres-introspect` feature was only ever passed by
//! `just test-pg`, which no workflow invoked at all). This crate's
//! `postgres-introspect` feature still isn't part of any workspace-wide
//! `just test-pg-tc` invocation by default (it's opt-in, per the
//! ticket's "no new DB dependency for the default build" requirement).
//!
//! Every table/view this file creates uses an `introspect_probe_`
//! prefix and is dropped (`IF EXISTS`) before creation, so tests are
//! safe to run repeatedly and — since each uses a distinct name — safe
//! to run concurrently against a shared database without a serializing
//! mutex.

use cratestack_migrate::emit::postgres::emit as emit_postgres;
use cratestack_migrate::introspect::postgres::introspect;
use cratestack_migrate::ir::{CheckKind, Column, ColumnArity, ColumnType};
use cratestack_migrate::{Projections, diff_projections, project};
use sqlx_core::pool::PoolOptions;
use sqlx_postgres::{PgPool, Postgres};
use testcontainers::ContainerAsync;
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres as PostgresContainer;

/// `pool` borrows a connection to whichever backend `connect_or_skip`
/// selected. `_container` is held only so the testcontainers backend's
/// container outlives every test that borrows `pool` from it — dropping
/// it early would tear the database down mid-test — and is never read
/// directly; it exists for its `Drop` (stops and removes the container).
struct TestPg {
    pool: PgPool,
    _container: Option<ContainerAsync<PostgresContainer>>,
}

async fn connect_or_skip() -> Option<TestPg> {
    let require = std::env::var("CRATESTACK_REQUIRE_DB").is_ok();

    fn need<T, E: std::fmt::Display>(r: Result<T, E>, require: bool, ctx: &str) -> Option<T> {
        match r {
            Ok(v) => Some(v),
            Err(e) if require => panic!("CRATESTACK_REQUIRE_DB is set but {ctx} failed: {e}"),
            Err(_) => None,
        }
    }

    if let Ok(url) = std::env::var("CRATESTACK_TEST_DATABASE_URL") {
        let pool = need(
            PoolOptions::<Postgres>::new()
                .max_connections(2)
                .connect(&url)
                .await,
            require,
            "connecting to CRATESTACK_TEST_DATABASE_URL",
        )?;
        return Some(TestPg {
            pool,
            _container: None,
        });
    }

    if std::env::var("CRATESTACK_USE_TESTCONTAINERS").is_ok() {
        let container = need(
            // Tag pinned explicitly: `testcontainers-modules` hardcodes
            // `postgres:11-alpine` as its default, EOL since 2023-11-09.
            // Kept in lockstep with `compose.yml`'s `postgres:18` so the
            // testcontainers backend (what CI runs) and the compose backend
            // (what `just test-pg` runs) introspect the same major.
            PostgresContainer::default()
                .with_tag("18-alpine")
                .start()
                .await,
            require,
            "starting the Postgres testcontainer (is Docker available?)",
        )?;
        let host = need(
            container.get_host().await,
            require,
            "resolving testcontainer host",
        )?;
        let port = need(
            container.get_host_port_ipv4(5432).await,
            require,
            "resolving testcontainer port",
        )?;
        let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
        let pool = need(
            PoolOptions::<Postgres>::new()
                .max_connections(2)
                .connect(&url)
                .await,
            require,
            "connecting to the Postgres testcontainer",
        )?;
        return Some(TestPg {
            pool,
            _container: Some(container),
        });
    }

    // cratestack#747 cited this as the correct reference implementation:
    // `cratestack-pg` and `cratestack-outbox` were missing exactly this
    // arm, so their whole PG-backed suites reported `ok` in 0.00s with
    // `CRATESTACK_REQUIRE_DB=1` set. Both now carry it, extracted into a
    // pure `pick_backend` under a `#[should_panic]` test — see
    // `crates/cratestack-pg/tests/support/require_db.rs`, which lists every
    // sibling copy. Left inline here because it is already correct.
    if require {
        panic!(
            "CRATESTACK_REQUIRE_DB is set but neither CRATESTACK_TEST_DATABASE_URL nor \
             CRATESTACK_USE_TESTCONTAINERS is set"
        );
    }

    None
}

async fn exec(pool: &PgPool, sql: &str) {
    // `AssertSqlSafe`: test-only DDL assembled from literals in this file
    // (sqlx 0.9's `SqlSafeStr` bound).
    sqlx_core::raw_sql::raw_sql(sqlx_core::sql_str::AssertSqlSafe(sql))
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
    let Some(test_pg) = connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool;
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
    let ops = diff_projections(&only_table(&report.projections, table), &expected)
        .expect("diff should succeed");
    assert!(ops.is_empty(), "expected no drift, got: {ops:?}");
}

/// A column type outside the common mapped set (`numeric`, `jsonb`,
/// arrays, …) is reported as unmapped, never guessed at — the design
/// doc's central safety rule.
#[tokio::test]
async fn unmapped_column_type_is_reported_not_guessed() {
    let Some(test_pg) = connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool;
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
    let Some(test_pg) = connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool;
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
    let Some(test_pg) = connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool;
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
    let Some(test_pg) = connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool;
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
    let Some(test_pg) = connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool;

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
    let Some(test_pg) = connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool;
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

/// cratestack#742's central risk and the ticket's decisive test:
/// applying a migration containing a partial (`where:`) unique index,
/// then re-planning against that same database, must produce **no**
/// ops for the index — not a spurious drop/recreate every run. A
/// hand-written IR fixture can't exercise this: the churn only shows up
/// once Postgres's own `pg_get_expr` deparse of the stored predicate
/// (verified empirically for cratestack#742 — see `crate::diff::indexes`'s
/// module doc for the exact normalization observed against a live
/// Postgres 18: whitespace collapsed, the whole predicate wrapped in
/// exactly one pair of parentheses) is compared against the schema's
/// literal `where:` text.
#[tokio::test]
async fn partial_unique_index_round_trips_without_churn() {
    let Some(test_pg) = connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool;
    let table = "introspection_probe_payments";

    let schema = cratestack_parser::parse_schema(
        r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model IntrospectionProbePayment {
  id String @id
  idempotencyKey String?
  amount Int

  @@unique([idempotencyKey], where: "idempotency_key IS NOT NULL")
}
"#,
    )
    .expect("hand-authored schema should parse");
    let expected = project(&schema);

    exec(&pool, &format!("DROP TABLE IF EXISTS {table}")).await;
    let create_ops = diff_projections(&Projections::default(), &only_table(&expected, table))
        .expect("diff should succeed");
    let migration = emit_postgres(&create_ops);
    assert!(!migration.has_lossy, "up was: {}", migration.up);
    assert!(
        migration.up.contains("WHERE idempotency_key IS NOT NULL"),
        "up was: {}",
        migration.up
    );
    exec(&pool, &migration.up).await;

    let report = introspect(&pool)
        .await
        .expect("introspection should succeed");
    let introspected_table = report
        .projections
        .tables
        .get(table)
        .unwrap_or_else(|| panic!("{table} should have been introspected"));

    // Prove the normalization the ticket asked us to confirm
    // empirically actually happened here too, not just against the
    // scratch database used to design `normalize_predicate` — the
    // introspected predicate is Postgres's own deparse, not the
    // schema's literal text.
    let introspected_predicates: Vec<Option<&str>> = introspected_table
        .indexes
        .iter()
        .map(|index| index.where_predicate.as_deref())
        .collect();
    assert_eq!(
        introspected_predicates,
        vec![Some("(idempotency_key IS NOT NULL)")],
        "pg_get_expr should have normalized the stored predicate"
    );

    // The decisive assertion: re-planning against the database the
    // migration was just applied to produces no ops for this table.
    let ops = diff_projections(&only_table(&report.projections, table), &expected)
        .expect("diff should succeed");
    assert!(
        ops.is_empty(),
        "re-planning a database whose partial index already matches the schema \
         must be a no-op — got: {ops:?}"
    );
}

/// cratestack#742 post-review remediation (Finding 1): the test above
/// only exercises `IS NOT NULL`, the one predicate shape immune to the
/// churn bug — it needs no literal, so Postgres inserts no `::type`
/// cast on introspection. This test uses the shape the finding named as
/// the actual production hazard: a text-literal comparison. Before the
/// fix, `status = 'active'` introspects as `(status = 'active'::text)`,
/// which never compared equal to the schema's literal text, so this
/// index dropped and recreated on every single `migrate` run.
#[tokio::test]
async fn partial_index_with_text_literal_predicate_round_trips_without_churn() {
    let Some(test_pg) = connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool;
    let table = cratestack_migrate::table_name("IntrospectionProbeOrder");

    let schema = cratestack_parser::parse_schema(
        r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model IntrospectionProbeOrder {
  id String @id
  status String

  @@index([status], where: "status = 'active'")
}
"#,
    )
    .expect("hand-authored schema should parse");
    let expected = project(&schema);

    exec(&pool, &format!("DROP TABLE IF EXISTS {table}")).await;
    let create_ops = diff_projections(&Projections::default(), &only_table(&expected, &table))
        .expect("diff should succeed");
    let migration = emit_postgres(&create_ops);
    assert!(!migration.has_lossy, "up was: {}", migration.up);
    assert!(
        migration.up.contains("WHERE status = 'active'"),
        "up was: {}",
        migration.up
    );
    exec(&pool, &migration.up).await;

    let report = introspect(&pool)
        .await
        .expect("introspection should succeed");
    let introspected_table = report
        .projections
        .tables
        .get(&table)
        .unwrap_or_else(|| panic!("{table} should have been introspected"));

    // Pin the exact cast Postgres inserts, so a future Postgres version
    // that stops doing this doesn't silently make this test meaningless.
    let introspected_predicates: Vec<Option<&str>> = introspected_table
        .indexes
        .iter()
        .map(|index| index.where_predicate.as_deref())
        .collect();
    assert_eq!(
        introspected_predicates,
        vec![Some("(status = 'active'::text)")],
        "pg_get_expr should have cast the text literal"
    );

    // The decisive assertion: re-planning against the database the
    // migration was just applied to produces no ops — this is the
    // finding's central claim, that without the fix this is NOT empty.
    let ops = diff_projections(&only_table(&report.projections, &table), &expected)
        .expect("diff should succeed");
    assert!(
        ops.is_empty(),
        "a text-literal partial index predicate must not churn on re-plan — got: {ops:?}"
    );
}

/// The numeric-literal counterpart of the text-literal test above:
/// `amount > 100` against a column whose type needs the literal cast to
/// a floating-point type introspects as
/// `(amount > (100)::double precision)` — a different cast shape
/// (parenthesized, no quotes, and — deliberately — a *multi-word* type
/// name) than the text case, exercising the other half of the minimum
/// bar the finding named.
///
/// This uses `Float` (Postgres `double precision`/`float8`), not the
/// `Decimal`/`NUMERIC` type the finding's own example used. That's a
/// deliberate substitution, not a narrowing of what's being proved:
/// verified empirically (a throwaway container, `psql`), a `NUMERIC`
/// column reproduces the exact `(amount > (100)::numeric)` shape the
/// finding named, but `NUMERIC` columns are *permanently* unmapped by
/// this crate's introspection regardless of this ticket
/// (`introspect::postgres::types::map_scalar` — "unmapped → reported
/// drift, never guessed", a pre-existing, deliberate design choice
/// unrelated to partial indexes: see that module's doc). A `Decimal`
/// column would make `ops.is_empty()` below fail on an unrelated
/// `AddColumn` for the column itself, on *every* run, which would prove
/// nothing about predicate-cast normalization one way or the other —
/// conflating this finding with a different, pre-existing gap. `Float`
/// triggers the identical cast-stripping code path (a parenthesized
/// literal immediately followed by `::<type>`) while its column type
/// *does* round-trip (`map_scalar("float8", ...) == Some("Float")`), so
/// this test can make the finding's actual "no churn" claim — an empty
/// re-plan — without also depending on fixing that unrelated gap. The
/// exact `NUMERIC` shape is pinned at the unit level instead, where it
/// doesn't need a real column to exist:
/// `diff::indexes::tests::strips_a_parenthesized_numeric_cast`.
#[tokio::test]
async fn partial_index_with_numeric_literal_predicate_round_trips_without_churn() {
    let Some(test_pg) = connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool;
    let table = cratestack_migrate::table_name("IntrospectionProbeTransaction");

    let schema = cratestack_parser::parse_schema(
        r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model IntrospectionProbeTransaction {
  id String @id
  amount Float

  @@index([amount], where: "amount > 100")
}
"#,
    )
    .expect("hand-authored schema should parse");
    let expected = project(&schema);

    exec(&pool, &format!("DROP TABLE IF EXISTS {table}")).await;
    let create_ops = diff_projections(&Projections::default(), &only_table(&expected, &table))
        .expect("diff should succeed");
    let migration = emit_postgres(&create_ops);
    assert!(!migration.has_lossy, "up was: {}", migration.up);
    assert!(
        migration.up.contains("WHERE amount > 100"),
        "up was: {}",
        migration.up
    );
    exec(&pool, &migration.up).await;

    let report = introspect(&pool)
        .await
        .expect("introspection should succeed");
    let introspected_table = report
        .projections
        .tables
        .get(&table)
        .unwrap_or_else(|| panic!("{table} should have been introspected"));

    let introspected_predicates: Vec<Option<&str>> = introspected_table
        .indexes
        .iter()
        .map(|index| index.where_predicate.as_deref())
        .collect();
    assert_eq!(
        introspected_predicates,
        vec![Some("(amount > (100)::double precision)")],
        "pg_get_expr should have cast the numeric literal to the column's floating-point type"
    );

    let ops = diff_projections(&only_table(&report.projections, &table), &expected)
        .expect("diff should succeed");
    assert!(
        ops.is_empty(),
        "a numeric-literal partial index predicate must not churn on re-plan — got: {ops:?}"
    );
}

/// The other half of the same requirement: a predicate that genuinely
/// changed must NOT be swallowed by the normalization-tolerant
/// comparison above — it has to still show up as a real drop + recreate.
#[tokio::test]
async fn partial_unique_index_predicate_change_is_detected_as_drop_and_recreate() {
    let Some(test_pg) = connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool;
    // Computed rather than hand-guessed: `table_name`'s pluralizer
    // doesn't just append `s` (cratestack#504 — consonant + `y` → `ies`,
    // and the plural is computed over the whole snake_cased name, not
    // per-word), so a literal here previously drifted from what
    // `project()` actually produced and made this test vacuous (it
    // diffed against an empty `Projections`, not the introspected
    // table).
    let table = cratestack_migrate::table_name("IntrospectionProbePaymentChanged");

    let original_source = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model IntrospectionProbePaymentChanged {
  id String @id
  idempotencyKey String?
  amount Int

  @@unique([idempotencyKey], where: "idempotency_key IS NOT NULL")
}
"#;
    let original_schema =
        cratestack_parser::parse_schema(original_source).expect("schema should parse");
    let original = project(&original_schema);

    exec(&pool, &format!("DROP TABLE IF EXISTS {table}")).await;
    let create_ops = diff_projections(&Projections::default(), &only_table(&original, &table))
        .expect("diff should succeed");
    exec(&pool, &emit_postgres(&create_ops).up).await;

    let changed_source = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model IntrospectionProbePaymentChanged {
  id String @id
  idempotencyKey String?
  amount Int

  @@unique([idempotencyKey], where: "idempotency_key IS NOT NULL AND amount > 0")
}
"#;
    let changed_schema =
        cratestack_parser::parse_schema(changed_source).expect("schema should parse");
    let changed = project(&changed_schema);

    let report = introspect(&pool)
        .await
        .expect("introspection should succeed");
    let ops = diff_projections(&only_table(&report.projections, &table), &changed)
        .expect("diff should succeed");

    let index_name = format!("{table}_idempotency_key_key");
    let drops_it = ops.iter().any(
        |op| matches!(op, cratestack_migrate::ir::Op::DropIndex(drop) if drop.name == index_name),
    );
    let re_adds_it = ops.iter().any(|op| {
        matches!(op, cratestack_migrate::ir::Op::AddIndex(add) if add.name == index_name
            && add.where_predicate.as_deref() == Some("idempotency_key IS NOT NULL AND amount > 0"))
    });
    assert!(
        drops_it && re_adds_it,
        "a changed predicate should drop + recreate the index — got: {ops:?}"
    );
}

/// cratestack#742 post-review remediation (Finding 2): introspecting
/// partial indexes at all (dropping the old `AND i.indpred IS NULL`
/// exclusion — see `introspect::postgres::indexes`'s module doc) widens
/// the blast radius of an existing, unrelated behavior: `diff_indexes`
/// emits a bare `DROP INDEX` for anything present in the database but
/// absent from the schema (`emit::postgres::indexes` — no `CASCADE`,
/// same as any other unmanaged index). Before cratestack#742, a
/// hand-made *partial* index outside Cratestack's management was
/// invisible to introspection and therefore never a drop candidate; now
/// it is, deliberately — see the module doc's rationale for why this is
/// treated the same as an ordinary unmanaged index rather than special-
/// cased. This test pins that this is not accidental: a `migrate` run
/// against a table carrying a hand-made partial index the schema never
/// declared emits `DropIndex` for it.
#[tokio::test]
async fn undeclared_partial_index_is_dropped_by_diff() {
    let Some(test_pg) = connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool;
    let table = cratestack_migrate::table_name("IntrospectionProbeLegacyOrder");
    let index_name = format!("{table}_legacy_status_partial_idx");

    exec(&pool, &format!("DROP TABLE IF EXISTS {table}")).await;
    exec(
        &pool,
        &format!(
            "CREATE TABLE {table} (
                id UUID NOT NULL PRIMARY KEY,
                status TEXT NOT NULL
            )"
        ),
    )
    .await;
    exec(
        &pool,
        &format!("CREATE INDEX {index_name} ON {table} (status) WHERE status = 'archived'"),
    )
    .await;

    // A schema for the same table that never mentions this index — the
    // "index created outside Cratestack" scenario that is #742's own
    // motivating example (see the ticket's Intent section).
    let schema = cratestack_parser::parse_schema(
        r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model IntrospectionProbeLegacyOrder {
  id String @id
  status String
}
"#,
    )
    .expect("hand-authored schema should parse");
    let expected = project(&schema);

    let report = introspect(&pool)
        .await
        .expect("introspection should succeed");
    let ops = diff_projections(
        &only_table(&report.projections, &table),
        &only_table(&expected, &table),
    )
    .expect("diff should succeed");

    let drops_it = ops.iter().any(
        |op| matches!(op, cratestack_migrate::ir::Op::DropIndex(drop) if drop.name == index_name),
    );
    assert!(
        drops_it,
        "an undeclared hand-made partial index must be a drop candidate, matching how an \
         undeclared ordinary index is already treated — got: {ops:?}"
    );
}

/// cratestack#742 post-review remediation, round 2 (Finding A): the
/// churn-tolerance fix from round 1 independently normalized each side
/// of a predicate before comparing them as plain strings, which threw
/// away the `::type` cast's own type name — so a predicate whose
/// **explicit** cast genuinely changed type (not just whether a cast was
/// present) compared as unchanged, and the database silently kept
/// enforcing the OLD uniqueness rule. `citext` (case-insensitive) vs.
/// `text` (case-sensitive) is the money-relevant version of this: an
/// author narrowing (or widening) a partial unique index's case
/// sensitivity by changing its cast must get a real migration, not
/// silence. Verified empirically (a throwaway container, `psql`) that
/// Postgres preserves the distinction in `pg_get_expr`'s deparse:
/// `email = 'x'::citext` against a `text` column reads back as
/// `(email = ('x'::citext)::text)` — a real, different cast, not folded
/// away — versus `(email = 'x'::text)` for the `::text` version.
#[tokio::test]
async fn partial_index_cast_type_change_is_detected_as_drop_and_recreate() {
    let Some(test_pg) = connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool;
    let table = cratestack_migrate::table_name("IntrospectionProbeAccount");

    exec(&pool, "CREATE EXTENSION IF NOT EXISTS citext").await;

    let original_source = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model IntrospectionProbeAccount {
  id String @id
  email String

  @@index([email], where: "email = 'admin@example.com'::citext")
}
"#;
    let original_schema =
        cratestack_parser::parse_schema(original_source).expect("schema should parse");
    let original = project(&original_schema);

    exec(&pool, &format!("DROP TABLE IF EXISTS {table}")).await;
    let create_ops = diff_projections(&Projections::default(), &only_table(&original, &table))
        .expect("diff should succeed");
    exec(&pool, &emit_postgres(&create_ops).up).await;

    let changed_source = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model IntrospectionProbeAccount {
  id String @id
  email String

  @@index([email], where: "email = 'admin@example.com'::text")
}
"#;
    let changed_schema =
        cratestack_parser::parse_schema(changed_source).expect("schema should parse");
    let changed = project(&changed_schema);

    let report = introspect(&pool)
        .await
        .expect("introspection should succeed");

    // Pin the exact, empirically-verified shape Postgres preserves —
    // if a future Postgres version stops distinguishing these, this
    // assertion (not just the decisive one below) will say so plainly.
    let introspected_table = report
        .projections
        .tables
        .get(&table)
        .unwrap_or_else(|| panic!("{table} should have been introspected"));
    let introspected_predicates: Vec<Option<&str>> = introspected_table
        .indexes
        .iter()
        .map(|index| index.where_predicate.as_deref())
        .collect();
    assert_eq!(
        introspected_predicates,
        vec![Some("(email = ('admin@example.com'::citext)::text)")],
        "pg_get_expr should preserve the citext cast, not fold it away"
    );

    let ops = diff_projections(&only_table(&report.projections, &table), &changed)
        .expect("diff should succeed");

    let index_name = format!("{table}_email_idx");
    let drops_it = ops.iter().any(
        |op| matches!(op, cratestack_migrate::ir::Op::DropIndex(drop) if drop.name == index_name),
    );
    let re_adds_it = ops.iter().any(|op| {
        matches!(op, cratestack_migrate::ir::Op::AddIndex(add) if add.name == index_name
            && add.where_predicate.as_deref() == Some("email = 'admin@example.com'::text"))
    });
    assert!(
        drops_it && re_adds_it,
        "a partial index whose predicate's explicit cast TYPE changed (citext -> text) must \
         drop + recreate — silently keeping the old index would mean the database keeps \
         enforcing case-insensitive uniqueness after the schema declared case-sensitive — got: \
         {ops:?}"
    );
}

/// cratestack#742 post-review remediation, round 3 (Finding D): once
/// both sides of a predicate carry an explicit `::type` cast, comparing
/// the type names by exact string equality (round 2's fix) missed that
/// Postgres normalizes an alias on deparse — an author-written `::int8`
/// round-trips through introspection as `::bigint`. Without alias
/// normalization, that would churn a drop+recreate on *every* `migrate`
/// run, forever, for anyone who happens to write an aliased spelling —
/// the same load-bearing "no churn" failure round 1 fixed for casts
/// entirely, resurfacing for a narrower population. Verified empirically
/// (a throwaway container, `psql`) that this specific pair is a clean
/// single-cast round-trip (the literal text itself is unchanged, unlike
/// e.g. a `varchar`-on-a-`text`-column cast, which Postgres nests behind
/// an extra implicit cast back to `text` and would churn for a
/// structural reason unrelated to this fix): `amount = '100'::int8`
/// against a `bigint` column reads back as `(amount = '100'::bigint)`.
#[tokio::test]
async fn partial_index_with_aliased_cast_type_round_trips_without_churn() {
    let Some(test_pg) = connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool;
    let table = cratestack_migrate::table_name("IntrospectionProbeLedgerEntry");

    let schema = cratestack_parser::parse_schema(
        r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model IntrospectionProbeLedgerEntry {
  id String @id
  amount Int

  @@index([amount], where: "amount = '100'::int8")
}
"#,
    )
    .expect("hand-authored schema should parse");
    let expected = project(&schema);

    exec(&pool, &format!("DROP TABLE IF EXISTS {table}")).await;
    let create_ops = diff_projections(&Projections::default(), &only_table(&expected, &table))
        .expect("diff should succeed");
    let migration = emit_postgres(&create_ops);
    assert!(!migration.has_lossy, "up was: {}", migration.up);
    assert!(
        migration.up.contains("WHERE amount = '100'::int8"),
        "up was: {}",
        migration.up
    );
    exec(&pool, &migration.up).await;

    let report = introspect(&pool)
        .await
        .expect("introspection should succeed");

    // Pin the exact, empirically-verified shape Postgres normalizes the
    // alias into — if a future Postgres version stops doing this, this
    // assertion (not just the decisive one below) will say so plainly.
    let introspected_table = report
        .projections
        .tables
        .get(&table)
        .unwrap_or_else(|| panic!("{table} should have been introspected"));
    let introspected_predicates: Vec<Option<&str>> = introspected_table
        .indexes
        .iter()
        .map(|index| index.where_predicate.as_deref())
        .collect();
    assert_eq!(
        introspected_predicates,
        vec![Some("(amount = '100'::bigint)")],
        "pg_get_expr should have normalized int8 to its bigint alias"
    );

    // The decisive assertion: re-planning against the database the
    // migration was just applied to produces no ops — without alias
    // normalization this would show a spurious drop+recreate on every
    // single run, forever.
    let ops = diff_projections(&only_table(&report.projections, &table), &expected)
        .expect("diff should succeed");
    assert!(
        ops.is_empty(),
        "an author-written aliased cast spelling must not churn on re-plan — got: {ops:?}"
    );
}
