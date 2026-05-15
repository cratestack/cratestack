//! End-to-end tests for tier-3 builder verbs against real Postgres:
//!
//!   * `FieldRef::eq_or_null(v)` matches column = v OR column IS NULL.
//!   * `FieldRef::match_optional(opt)` skips the filter entirely on
//!     `None`, falls through to `eq_or_null` on `Some`.
//!   * `coalesce(cols).<cmp>(value)` renders as `COALESCE(...) <cmp>
//!     value`.
//!   * `.where_optional(...)` no-ops on `None` inputs.

mod support;

use cratestack::include_server_schema;
use cratestack::sqlx::query;
use cratestack::{coalesce, CoolContext, FieldRef, Value};
use support::pg;

include_server_schema!("tests/fixtures/builder_extensions_tier3.cstack", db = Postgres);

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS task_rows")
        .execute(pool)
        .await
        .expect("drop tables");
    query(
        "CREATE TABLE task_rows (
            id BIGINT PRIMARY KEY,
            market_code TEXT,
            next_attempt_at BIGINT,
            scheduled_at BIGINT,
            created_at BIGINT NOT NULL
        )",
    )
    .execute(pool)
    .await
    .expect("create task_rows");
}

fn operator() -> CoolContext {
    CoolContext::authenticated([("id".to_owned(), Value::Int(1))])
        .with_request_id("tier3-001")
}

async fn seed(pool: &cratestack::sqlx::PgPool) {
    // (id, market_code, next_attempt_at, scheduled_at, created_at)
    let rows: &[(i64, Option<&str>, Option<i64>, Option<i64>, i64)] = &[
        (1, Some("us"), Some(100), None, 1),
        (2, Some("eu"), None, Some(200), 2),
        (3, None, None, None, 5),
    ];
    for (id, mc, na, sched, created) in rows {
        query(
            "INSERT INTO task_rows (id, market_code, next_attempt_at, scheduled_at, created_at) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(id)
        .bind(*mc)
        .bind(*na)
        .bind(*sched)
        .bind(created)
        .execute(pool)
        .await
        .expect("seed row");
    }
}

// ───── #7 eq_or_null + match_optional + where_optional ───────────────────────

#[tokio::test]
async fn eq_or_null_matches_value_and_null_rows() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    use cratestack_schema::task_row;
    let hits = cool
        .task_row()
        .bind(ctx)
        .find_many()
        .where_(task_row::marketCode().eq_or_null("us"))
        .run()
        .await
        .expect("query succeeds");
    let codes: Vec<Option<String>> = hits.iter().map(|t| t.marketCode.clone()).collect();
    // Matches "us" + the null-market row.
    assert_eq!(codes.len(), 2);
    assert!(codes.iter().any(|c| c.as_deref() == Some("us")));
    assert!(codes.iter().any(|c| c.is_none()));
}

#[tokio::test]
async fn match_optional_none_skips_the_filter_entirely() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    use cratestack_schema::task_row;
    let user_input: Option<&str> = None;
    let hits = cool
        .task_row()
        .bind(ctx)
        .find_many()
        .where_optional(task_row::marketCode().match_optional(user_input))
        .run()
        .await
        .unwrap();
    assert_eq!(hits.len(), 3, "no filter applied → all rows");
}

// ───── #13 coalesce ──────────────────────────────────────────────────────────

#[tokio::test]
async fn coalesce_lte_selects_earliest_non_null_time() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    // COALESCE(next_attempt_at, scheduled_at, created_at) <= 100:
    //   row 1: 100  ≤ 100 ✓
    //   row 2: 200  ≤ 100 ✗
    //   row 3: 5    ≤ 100 ✓
    let hits = cool
        .task_row()
        .bind(ctx)
        .find_many()
        .where_expr(
            coalesce([
                FieldRef::<cratestack_schema::TaskRow, Option<i64>>::new("next_attempt_at")
                    .column_name(),
                FieldRef::<cratestack_schema::TaskRow, Option<i64>>::new("scheduled_at")
                    .column_name(),
                FieldRef::<cratestack_schema::TaskRow, i64>::new("created_at").column_name(),
            ])
            .lte(100_i64),
        )
        .run()
        .await
        .unwrap();
    let ids: Vec<i64> = hits.iter().map(|t| t.id).collect();
    assert_eq!(ids.len(), 2, "expected rows 1 and 3, got: {ids:?}");
    assert!(ids.contains(&1));
    assert!(ids.contains(&3));
}

#[tokio::test]
async fn coalesce_accepts_bare_str_columns() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();
    // Same query, plain string column names — the `IntoColumnName`
    // impl on `&'static str` keeps this ergonomic when call sites
    // can't easily get hold of a typed `FieldRef`.
    let hits = cool
        .task_row()
        .bind(ctx)
        .find_many()
        .where_expr(
            coalesce(["next_attempt_at", "scheduled_at", "created_at"]).lte(100_i64),
        )
        .run()
        .await
        .unwrap();
    assert_eq!(hits.len(), 2);
}

#[tokio::test]
async fn coalesce_is_null_returns_zero_rows_when_a_column_is_not_null() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();
    // `created_at` is NOT NULL — every row has at least one non-null
    // value in the coalesce tuple, so IS NULL matches nothing.
    let hits = cool
        .task_row()
        .bind(ctx)
        .find_many()
        .where_expr(coalesce(["next_attempt_at", "scheduled_at", "created_at"]).is_null())
        .run()
        .await
        .unwrap();
    assert!(hits.is_empty());
}
