//! End-to-end tests for `.include(...)` to-one relation side-loading
//! against real Postgres.
//!
//! Hand-built `RelationInclude` in v1 (codegen sugar is a follow-up):
//! the test extracts the FK from the parent via a function pointer
//! into the generated `Delivery` struct and points at the generated
//! `SUBSCRIPTION_MODEL` descriptor.

mod support;

use cratestack::include_server_schema;
use cratestack::sqlx::query;
use cratestack::{CoolContext, RelationInclude, Value};
use support::pg;

include_server_schema!(
    "tests/fixtures/builder_extensions_tier6.cstack",
    db = Postgres
);

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS deliverys, subscriptions")
        .execute(pool)
        .await
        .expect("drop tables");
    query("CREATE TABLE subscriptions (id BIGINT PRIMARY KEY, label TEXT NOT NULL)")
        .execute(pool)
        .await
        .expect("create subscriptions");
    query(
        "CREATE TABLE deliverys (
            id BIGINT PRIMARY KEY,
            subscription_id BIGINT,
            label TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await
    .expect("create deliverys");
}

fn operator() -> CoolContext {
    CoolContext::authenticated([("id".to_owned(), Value::Int(1))]).with_request_id("tier6-001")
}

async fn seed(pool: &cratestack::sqlx::PgPool) {
    for (id, label) in &[(1_i64, "a"), (2_i64, "b")] {
        query("INSERT INTO subscriptions (id, label) VALUES ($1, $2)")
            .bind(id)
            .bind(*label)
            .execute(pool)
            .await
            .unwrap();
    }
    let rows: &[(i64, Option<i64>, &str)] = &[
        (10, Some(1), "d1-to-a"),
        (11, Some(1), "d2-to-a"),
        (12, Some(2), "d3-to-b"),
        (13, None, "d4-orphan"),       // null FK
        (14, Some(99), "d5-dangling"), // FK references missing subscription
    ];
    for (id, sub, label) in rows {
        query("INSERT INTO deliverys (id, subscription_id, label) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(*sub)
            .bind(*label)
            .execute(pool)
            .await
            .unwrap();
    }
}

fn subscription_relation()
-> RelationInclude<cratestack_schema::Delivery, cratestack_schema::Subscription, i64> {
    RelationInclude {
        parent_fk_extract: |d: &cratestack_schema::Delivery| d.subscriptionId,
        related_descriptor: &cratestack_schema::models::SUBSCRIPTION_MODEL,
    }
}

#[tokio::test]
async fn include_resolves_matched_relations_in_one_extra_query() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();
    use cratestack_schema::delivery;

    let pairs = cool
        .delivery()
        .bind(ctx)
        .find_many()
        .include(subscription_relation())
        .order_by(delivery::id().asc())
        .run()
        .await
        .expect("include round-trip");

    let summary: Vec<(i64, Option<String>)> = pairs
        .iter()
        .map(|(d, s)| (d.id, s.as_ref().map(|s| s.label.clone())))
        .collect();
    assert_eq!(
        summary,
        vec![
            (10_i64, Some("a".to_string())),
            (11, Some("a".to_string())),
            (12, Some("b".to_string())),
            (13, None),
            (14, None),
        ],
    );
}

#[tokio::test]
async fn include_filters_apply_to_parent_only_not_related() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();
    use cratestack_schema::delivery;

    let pairs = cool
        .delivery()
        .bind(ctx)
        .find_many()
        .include(subscription_relation())
        .where_(delivery::subscriptionId().eq(1_i64))
        .order_by(delivery::id().asc())
        .run()
        .await
        .unwrap();

    assert_eq!(pairs.len(), 2);
    assert!(pairs.iter().all(|(_, s)| s.as_ref().unwrap().id == 1));
}

#[tokio::test]
async fn include_with_zero_parents_skips_side_load() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();
    use cratestack_schema::delivery;

    let pairs = cool
        .delivery()
        .bind(ctx)
        .find_many()
        .include(subscription_relation())
        .where_(delivery::label().eq("nope"))
        .run()
        .await
        .unwrap();
    assert!(pairs.is_empty());
}
