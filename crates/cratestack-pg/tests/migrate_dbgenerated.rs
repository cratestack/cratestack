//! End-to-end coverage for issue #138 against a real Postgres.
//!
//! `@default(dbgenerated())` is a marker, not a value — see
//! `cratestack_migrate::ir::ColumnDefault::DbGenerated`. This test
//! applies `cratestack-migrate`'s actual emitted DDL (not hand-written
//! SQL, unlike the other `policy_db_*` fixtures) and confirms the
//! chosen semantics end-to-end:
//! - the DDL never contains the invalid literal `DEFAULT dbgenerated()`;
//! - a `dbgenerated()` column with no real Postgres-level default
//!   fails clearly (a `NOT NULL` violation) when an insert omits it;
//! - once a real default is set some other way (mirroring how
//!   `policy_db_auth_engine.rs` / `policy_db_recursive.rs` provision
//!   their `dbgenerated()` id columns), the same insert succeeds.

mod support;

use cratestack::sqlx::query;
use cratestack::{Migration, apply_pending};
use cratestack_migrate::diff;
use cratestack_migrate::emit::postgres;
use cratestack_parser::parse_schema;
use support::pg;

const SCHEMA: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model DbgeneratedProbe {
  id String @id @default(dbgenerated())
  title String
}
"#;

async fn reset(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS cratestack_migrations, dbgenerated_probes")
        .execute(pool)
        .await
        .expect("drop");
}

#[tokio::test]
async fn dbgenerated_probe_migration_and_insert_behavior() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset(pool).await;

    let empty = parse_schema("").expect("empty schema should parse");
    let next = parse_schema(SCHEMA).expect("probe schema should parse");
    let migration = postgres::emit(&diff(&empty, &next));

    // Bug 1: the emitted DDL must never contain the invalid literal.
    assert!(
        !migration.up.contains("DEFAULT dbgenerated()"),
        "emitted DDL must never contain the literal `DEFAULT dbgenerated()`: {}",
        migration.up
    );
    // Bug 2: the diff engine must flag this column so callers (here,
    // `cratestack migrate diff`) can warn before a runtime failure.
    assert_eq!(
        migration.unverified_dbgenerated,
        vec![("dbgenerated_probes".to_owned(), "id".to_owned())],
    );

    apply_pending(
        pool,
        &[Migration {
            id: "20260722000000_dbgenerated_probe".to_owned(),
            description: "dbgenerated probe".to_owned(),
            up: migration.up.clone(),
            down: None,
        }],
    )
    .await
    .expect("emitted DDL must apply cleanly against real Postgres");

    // No real Postgres-level default exists yet — an insert that
    // omits `id` must fail clearly with a NOT NULL violation, not
    // silently succeed or hang.
    let insert_without_default = query("INSERT INTO dbgenerated_probes (title) VALUES ('first')")
        .execute(pool)
        .await;
    let error = insert_without_default.expect_err(
        "inserting without a value for a dbgenerated() column with no real default \
         must fail",
    );
    assert!(
        error.to_string().contains("null value")
            && error.to_string().contains("not-null constraint"),
        "expected a NOT NULL violation, got: {error}",
    );

    // Now give the column a real Postgres-level default some other
    // way (hand-authored SQL) — exactly the scenario `dbgenerated()`
    // is meant to describe. The same insert must now succeed.
    query("ALTER TABLE dbgenerated_probes ALTER COLUMN id SET DEFAULT (md5(random()::text))")
        .execute(pool)
        .await
        .expect("set a real default by hand");

    query("INSERT INTO dbgenerated_probes (title) VALUES ('second')")
        .execute(pool)
        .await
        .expect("insert should succeed once a real default exists");

    let count: (i64,) =
        cratestack::sqlx::query_as("SELECT COUNT(*)::BIGINT FROM dbgenerated_probes")
            .fetch_one(pool)
            .await
            .expect("count rows");
    assert_eq!(count.0, 1, "only the successful insert should have landed");
}
