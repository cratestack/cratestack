//! An outbox table an earlier version created has no `payload_before`. An
//! update of an `@@subscribe` model upgrades it, outside its transaction and
//! on the table the `INSERT` will use; and nothing that merely reads the
//! outbox (the drain, the large-event read behind a notification) needs the
//! upgrade, so a table that could not be upgraded still works.

use std::time::Duration;

use cratestack::serde_json::json;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{CratestackContext, SubscriptionDispatcher, SubscriptionOpen, Value};

use super::cratestack_schema::{self, Cratestack};
use super::support::pg;

const SUB_NOTES: &str = "CREATE TABLE sub_notes (id BIGINT PRIMARY KEY, owner_id BIGINT NOT NULL, body TEXT NOT NULL, archived BOOLEAN NOT NULL)";
const LEGACY_OUTBOX: &str = "CREATE TABLE cratestack_event_outbox (event_id UUID PRIMARY KEY, model TEXT NOT NULL, operation TEXT NOT NULL, occurred_at TIMESTAMPTZ NOT NULL, payload JSONB NOT NULL, delivered_at TIMESTAMPTZ, attempts BIGINT NOT NULL DEFAULT 0, last_error TEXT)";

fn caller() -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::Int(1))])
}

async fn run(pool: &cratestack::sqlx::PgPool, statements: &[&'static str]) {
    for statement in statements {
        cratestack::sqlx::query(*statement)
            .execute(pool)
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }
}

async fn has_column(pool: &cratestack::sqlx::PgPool, table: &str) -> bool {
    cratestack::sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_attribute WHERE attrelid = $1::regclass \
         AND attname = 'payload_before' AND NOT attisdropped)",
    )
    .bind(table)
    .fetch_one(pool)
    .await
    .expect("catalog")
}

async fn create(db: &Cratestack, id: i64, body: &str) {
    db.sub_note()
        .create(cratestack_schema::CreateSubNoteInput {
            id,
            ownerId: 1,
            body: body.to_owned(),
            archived: false,
        })
        .run(&caller())
        .await
        .expect("created");
}

async fn update(db: &Cratestack, id: i64, body: &str) {
    db.sub_note()
        .update(id)
        .set(cratestack_schema::UpdateSubNoteInput {
            ownerId: None,
            body: Some(body.to_owned()),
            archived: None,
        })
        .run(&caller())
        .await
        .expect("updated");
}

#[tokio::test]
async fn an_old_outbox_table_gains_its_column_and_keeps_the_pre_image() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let _guard = pg::serial_guard().await;
    run(
        &test_pg.pool,
        &[
            "DROP TABLE IF EXISTS sub_notes",
            SUB_NOTES,
            "DROP TABLE IF EXISTS cratestack_event_outbox",
            LEGACY_OUTBOX,
        ],
    )
    .await;
    let db = Cratestack::builder(test_pg.pool.clone()).build();
    create(&db, 1, "keep me").await;
    // A single update opens its own transaction: the upgrade comes first.
    update(&db, 1, "drop me").await;
    let stored: (i64,) = cratestack::sqlx::query_as(
        "SELECT count(*) FROM cratestack_event_outbox WHERE payload_before ->> 'body' = 'keep me'",
    )
    .fetch_one(&test_pg.pool)
    .await
    .expect("the column exists and holds the pre-image");
    assert_eq!(stored.0, 1);
}

#[tokio::test]
async fn the_upgrade_follows_the_search_path_of_the_insert() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let _guard = pg::serial_guard().await;
    run(
        &test_pg.pool,
        &[
            "DROP SCHEMA IF EXISTS tenant_a CASCADE",
            "CREATE SCHEMA tenant_a",
            "DROP TABLE IF EXISTS public.sub_notes, public.cratestack_event_outbox",
            // `public` is already current: a check against it alone would pass.
            "CREATE TABLE public.cratestack_event_outbox (event_id UUID PRIMARY KEY, model TEXT NOT NULL, operation TEXT NOT NULL, occurred_at TIMESTAMPTZ NOT NULL, payload JSONB NOT NULL, payload_before JSONB, delivered_at TIMESTAMPTZ, attempts BIGINT NOT NULL DEFAULT 0, last_error TEXT)",
            "CREATE TABLE tenant_a.cratestack_event_outbox (event_id UUID PRIMARY KEY, model TEXT NOT NULL, operation TEXT NOT NULL, occurred_at TIMESTAMPTZ NOT NULL, payload JSONB NOT NULL, delivered_at TIMESTAMPTZ, attempts BIGINT NOT NULL DEFAULT 0, last_error TEXT)",
            "CREATE TABLE tenant_a.sub_notes (id BIGINT PRIMARY KEY, owner_id BIGINT NOT NULL, body TEXT NOT NULL, archived BOOLEAN NOT NULL)",
        ],
    )
    .await;
    let tenant_pool = PgPoolOptions::new()
        .max_connections(3)
        .after_connect(|connection, _| {
            Box::pin(async move {
                cratestack::sqlx::query("SET search_path = tenant_a, public")
                    .execute(&mut *connection)
                    .await?;
                Ok(())
            })
        })
        .connect(&test_pg.url)
        .await
        .expect("a pool on the tenant's search_path");
    let db = Cratestack::builder(tenant_pool).build();
    create(&db, 1, "keep me").await;
    update(&db, 1, "drop me").await;
    assert!(has_column(&test_pg.pool, "tenant_a.cratestack_event_outbox").await);
    run(&test_pg.pool, &["DROP SCHEMA tenant_a CASCADE"]).await;
}

/// A reader holds the outbox table, so the upgrade's `ALTER` gives up after
/// 100 ms: the table stays as it was, and neither the drain nor a large event
/// (whose notification refers to the outbox row, read back by the listener)
/// may name the missing column.
#[tokio::test]
async fn a_table_that_could_not_be_upgraded_still_drains_and_serves_large_events() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let _guard = pg::serial_guard().await;
    run(
        &test_pg.pool,
        &[
            "DROP TABLE IF EXISTS sub_notes",
            SUB_NOTES,
            "DROP TABLE IF EXISTS cratestack_event_outbox",
            LEGACY_OUTBOX,
        ],
    )
    .await;
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&test_pg.url)
        .await
        .expect("a pool with room for the listener and a reader");
    let mut reader = pool.begin().await.expect("reader");
    cratestack::sqlx::query("SELECT 1 FROM cratestack_event_outbox")
        .execute(&mut *reader)
        .await
        .expect("an ACCESS SHARE lock");
    let db = Cratestack::builder(pool.clone()).build();
    let mut feed = db
        .subscriptions()
        .open(
            SubscriptionOpen {
                op_id: "model.SubNote.subscribe".to_owned(),
                input: json!({}),
                ctx: caller(),
            },
            cratestack::JsonCodec,
        )
        .await
        .expect("the feed opens");
    // Over Postgres' 8000-byte NOTIFY limit: the listener reads the row.
    let large = "x".repeat(10_000);
    create(&db, 1, &large).await;
    update(&db, 1, "small").await;

    let mut bodies = Vec::new();
    while let Ok(Some(Ok(event))) = tokio::time::timeout(
        Duration::from_millis(1500),
        cratestack::futures::StreamExt::next(&mut feed),
    )
    .await
    {
        let event = cratestack::serde_json::Value::from(event);
        bodies.push(event["data"]["body"].as_str().map(str::len));
    }
    assert_eq!(bodies, [Some(10_000), Some(5)], "both events arrive");
    assert!(!has_column(&test_pg.pool, "cratestack_event_outbox").await);
    // Nothing is left undelivered by the drain that each write ran.
    let pending: (i64,) = cratestack::sqlx::query_as(
        "SELECT count(*) FROM cratestack_event_outbox WHERE delivered_at IS NULL",
    )
    .fetch_one(&test_pg.pool)
    .await
    .expect("count");
    assert_eq!(pending.0, 0);
    reader.commit().await.expect("release");
}
