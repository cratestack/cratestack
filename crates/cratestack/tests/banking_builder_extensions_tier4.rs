//! End-to-end tests for tier-4 builder verbs against real Postgres:
//!
//!   * `#4` — `.aggregate().count()/sum/avg/min/max(...)` returning
//!     scalars filtered through the read policy.
//!   * `#6` — `delete_many(filter)` returning `BatchSummary`, with
//!     per-row audit + outbox fan-out via `RETURNING`.
//!   * `#11` — `.order_by(col.asc().nulls_first())` placement override.

mod support;

use cratestack::include_server_schema;
use cratestack::sqlx::{Row, query};
use cratestack::{CoolContext, Value};
use support::pg;

include_server_schema!("tests/fixtures/builder_extensions_tier4.cstack", db = Postgres);

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS cratestack_audit, cratestack_event_outbox, refunds")
        .execute(pool)
        .await
        .expect("drop tables");
    query(
        "CREATE TABLE refunds (
            id BIGINT PRIMARY KEY,
            payment_intent_id TEXT NOT NULL,
            status TEXT NOT NULL,
            amount_minor BIGINT NOT NULL
        )",
    )
    .execute(pool)
    .await
    .expect("create refunds");
}

fn operator() -> CoolContext {
    CoolContext::authenticated([("id".to_owned(), Value::Int(1))])
        .with_request_id("tier4-001")
}

async fn seed(pool: &cratestack::sqlx::PgPool) {
    // (id, payment_intent_id, status, amount_minor)
    let rows: &[(i64, &str, &str, i64)] = &[
        (1, "pi_A", "succeeded", 1000),
        (2, "pi_A", "succeeded", 2500),
        (3, "pi_A", "failed", 5000),
        (4, "pi_B", "succeeded", 700),
    ];
    for (id, pi, status, amount) in rows {
        query(
            "INSERT INTO refunds (id, payment_intent_id, status, amount_minor) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(id)
        .bind(*pi)
        .bind(*status)
        .bind(amount)
        .execute(pool)
        .await
        .expect("seed row");
    }
}

// ───── #4 aggregate ──────────────────────────────────────────────────────────

#[tokio::test]
async fn aggregate_sum_filters_through_read_policy() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    use cratestack_schema::refund;
    // Sum of non-failed refunds on pi_A — the named site in the
    // upstream typed migration. Should yield 1000 + 2500 = 3500.
    // PG returns `SUM(BIGINT)` as `NUMERIC`, so callers decode into
    // a Decimal-compatible scalar; for `INT` columns it decodes
    // straight into `Option<i64>`. The framework doesn't paper over
    // this — the call site picks the scalar type that matches PG's
    // promotion rules for the column.
    let sum: Option<cratestack::Decimal> = cool
        .refund()
        .bind(ctx.clone())
        .aggregate()
        .sum(refund::amountMinor())
        .where_(refund::paymentIntentId().eq("pi_A"))
        .where_(refund::status().ne("failed"))
        .run()
        .await
        .expect("aggregate sum succeeds");
    assert_eq!(
        sum,
        Some(cratestack::Decimal::from(3500_i64)),
    );
}

#[tokio::test]
async fn aggregate_count_returns_zero_for_empty_match() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    use cratestack_schema::refund;
    let zero = cool
        .refund()
        .bind(ctx)
        .aggregate()
        .count()
        .where_(refund::paymentIntentId().eq("pi_NONE"))
        .run()
        .await
        .unwrap();
    assert_eq!(zero, 0, "count returns 0 (not None) on empty match");
}

#[tokio::test]
async fn aggregate_min_max_round_trip() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    use cratestack_schema::refund;
    let min: Option<i64> = cool
        .refund()
        .bind(ctx.clone())
        .aggregate()
        .min(refund::amountMinor())
        .run()
        .await
        .unwrap();
    let max: Option<i64> = cool
        .refund()
        .bind(ctx)
        .aggregate()
        .max(refund::amountMinor())
        .run()
        .await
        .unwrap();
    assert_eq!(min, Some(700));
    assert_eq!(max, Some(5000));
}

// ───── #6 delete_many ────────────────────────────────────────────────────────

#[tokio::test]
async fn delete_many_removes_matching_rows_and_writes_audit_per_row() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    use cratestack_schema::refund;
    let summary = cool
        .refund()
        .bind(ctx)
        .delete_many()
        .where_(refund::status().eq("failed"))
        .run()
        .await
        .expect("delete_many succeeds");
    assert_eq!(summary.total, 1);
    assert_eq!(summary.ok, 1);

    let surviving: i64 = query("SELECT COUNT(*) FROM refunds")
        .fetch_one(pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(surviving, 3, "three non-failed refunds remain");

    let audit: i64 = query(
        "SELECT COUNT(*) FROM cratestack_audit WHERE model = 'Refund' AND operation = 'delete'",
    )
    .fetch_one(pool)
    .await
    .unwrap()
    .get(0);
    assert_eq!(audit, 1, "one delete audit row per affected refund");
}

#[tokio::test]
async fn delete_many_refuses_without_filter() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();
    let err = cool
        .refund()
        .bind(ctx)
        .delete_many()
        .run()
        .await
        .expect_err("predicate-less delete_many must fail");
    let detail = err.detail().unwrap_or_default();
    assert!(detail.contains("at least one filter"), "got: {detail:?}");
}

// ───── #11 NULLS FIRST / NULLS LAST ──────────────────────────────────────────

#[tokio::test]
async fn order_by_nulls_first_preview_includes_nulls_first_clause() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    use cratestack_schema::refund;
    let preview = cool
        .refund()
        .bind(ctx.clone())
        .find_many()
        .order_by(refund::status().asc().nulls_first())
        .preview_scoped_sql();
    assert!(
        preview.contains("status ASC NULLS FIRST"),
        "got: {preview}",
    );

    // Sanity-check the live query: it should also run cleanly with the
    // NULLS FIRST clause emitted.
    let rows = cool
        .refund()
        .bind(ctx)
        .find_many()
        .order_by(refund::status().asc().nulls_first())
        .run()
        .await
        .expect("find_many with nulls_first runs");
    assert_eq!(rows.len(), 4);
}
