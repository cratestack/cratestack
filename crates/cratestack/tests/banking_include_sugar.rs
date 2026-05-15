//! End-to-end test for the codegen sugar
//! `<model_module>::<relation>().as_include()`.
//!
//! Reuses the tier-6 fixture (`Subscription` parent + `Delivery`
//! child with an optional FK). The earlier tier-6 PR shipped the
//! runtime `FindMany::include(...)` taking a hand-built
//! `RelationInclude` literal; this follow-up adds the typed
//! accessor so call sites can write:
//!
//! ```ignore
//! .include(delivery::subscription().as_include())
//! ```
//!
//! instead of:
//!
//! ```ignore
//! .include(RelationInclude {
//!     parent_fk_extract: |d: &Delivery| d.subscriptionId,
//!     related_descriptor: &models::SUBSCRIPTION_MODEL,
//! })
//! ```

mod support;

use cratestack::include_server_schema;
use cratestack::sqlx::query;
use cratestack::{CoolContext, Value};
use support::pg;

include_server_schema!("tests/fixtures/builder_extensions_tier6.cstack", db = Postgres);

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
    CoolContext::authenticated([("id".to_owned(), Value::Int(1))])
        .with_request_id("include-sugar-001")
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
        (11, Some(2), "d2-to-b"),
        (12, None, "d3-orphan"),
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

#[tokio::test]
async fn include_via_codegen_sugar_resolves_relations() {
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
        // The codegen sugar: `subscription()` is the relation Path,
        // `.as_include()` produces the `RelationInclude` from the
        // schema-known FK metadata.
        .include(delivery::subscription().as_include())
        .order_by(delivery::id().asc())
        .run()
        .await
        .expect("include succeeds");

    let summary: Vec<(i64, Option<String>)> = pairs
        .iter()
        .map(|(d, s)| (d.id, s.as_ref().map(|s| s.label.clone())))
        .collect();
    assert_eq!(
        summary,
        vec![
            (10_i64, Some("a".to_string())),
            (11, Some("b".to_string())),
            (12, None),
        ],
    );
}
