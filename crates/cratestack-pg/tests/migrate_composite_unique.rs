//! End-to-end coverage for issue #262 against a real Postgres.
//!
//! A model-level `@@unique([...])` has to reach the database as a real
//! unique index, not just as an IR op that reads correctly in a unit
//! test. Two things only a live server can confirm:
//! - the emitted DDL applies, and rejects a duplicate tuple;
//! - `ON CONFLICT (a, b, c) DO UPDATE` resolves against it. Postgres
//!   requires an existing unique index matching the conflict target
//!   exactly, so the idempotent-upsert pattern the issue was filed from
//!   fails with "no unique or exclusion constraint matching the ON
//!   CONFLICT specification" until this DDL exists.
//!
//! Follows `migrate_dbgenerated.rs`: the SQL under test is
//! `cratestack-migrate`'s own emitted output, never hand-written DDL.

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

model UniqueProbe {
  id String @id
  tenantId String
  scopeId String
  reportDay String
  total Int

  @@unique([tenantId, scopeId, reportDay])
}
"#;

async fn reset(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS cratestack_migrations, unique_probes")
        .execute(pool)
        .await
        .expect("drop");
}

#[tokio::test]
async fn composite_unique_reaches_postgres_and_serves_on_conflict() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset(pool).await;

    let empty = parse_schema("").expect("empty schema should parse");
    let next = parse_schema(SCHEMA).expect("probe schema should parse");
    let migration = postgres::emit(&diff(&empty, &next).expect("diff should succeed"));

    assert!(
        migration.up.contains(
            "CREATE UNIQUE INDEX unique_probes_tenant_id_scope_id_report_day_key \
             ON unique_probes (tenant_id, scope_id, report_day);"
        ),
        "emitted DDL must carry the composite unique index: {}",
        migration.up
    );

    apply_pending(
        pool,
        &[Migration {
            id: "20260802000000_unique_probe".to_owned(),
            description: "unique probe".to_owned(),
            up_pre: None,
            up: migration.up.clone(),
            down: None,
        }],
    )
    .await
    .expect("emitted DDL must apply cleanly against real Postgres");

    // Postgres agrees the index exists, is unique, and covers exactly
    // the declared tuple in declaration order.
    let indexed: (String,) = cratestack::sqlx::query_as(
        "SELECT indexdef FROM pg_indexes \
         WHERE tablename = 'unique_probes' \
           AND indexname = 'unique_probes_tenant_id_scope_id_report_day_key'",
    )
    .fetch_one(pool)
    .await
    .expect("the composite unique index must exist in the catalog");
    assert!(
        indexed.0.contains("CREATE UNIQUE INDEX")
            && indexed.0.contains("(tenant_id, scope_id, report_day)"),
        "unexpected index definition: {}",
        indexed.0
    );

    query(
        "INSERT INTO unique_probes (id, tenant_id, scope_id, report_day, total) \
         VALUES ('a', 't1', 's1', '2026-08-02', 1)",
    )
    .execute(pool)
    .await
    .expect("first row inserts");

    // Same tuple, different primary key — must be rejected.
    let duplicate = query(
        "INSERT INTO unique_probes (id, tenant_id, scope_id, report_day, total) \
         VALUES ('b', 't1', 's1', '2026-08-02', 2)",
    )
    .execute(pool)
    .await
    .expect_err("a duplicate (tenant_id, scope_id, report_day) tuple must be rejected");
    assert!(
        duplicate
            .to_string()
            .contains("unique_probes_tenant_id_scope_id_report_day_key"),
        "expected a unique violation naming the index, got: {duplicate}",
    );

    // The motivating case: an idempotent upsert targeting the tuple.
    for total in [10_i64, 20_i64] {
        query(
            "INSERT INTO unique_probes (id, tenant_id, scope_id, report_day, total) \
             VALUES ('c', 't1', 's1', '2026-08-02', $1) \
             ON CONFLICT (tenant_id, scope_id, report_day) \
             DO UPDATE SET total = EXCLUDED.total",
        )
        .bind(total)
        .execute(pool)
        .await
        .expect("ON CONFLICT must resolve against the composite unique index");
    }

    let row: (i64, i64) = cratestack::sqlx::query_as(
        "SELECT COUNT(*)::BIGINT, MAX(total)::BIGINT FROM unique_probes",
    )
    .fetch_one(pool)
    .await
    .expect("count rows");
    assert_eq!(row.0, 1, "the upsert must update in place, not insert");
    assert_eq!(row.1, 20, "the last upsert's value must win");

    // A different tuple is unaffected by the constraint.
    query(
        "INSERT INTO unique_probes (id, tenant_id, scope_id, report_day, total) \
         VALUES ('d', 't1', 's1', '2026-08-03', 1)",
    )
    .execute(pool)
    .await
    .expect("a distinct tuple must still insert");

    reset(pool).await;
}
