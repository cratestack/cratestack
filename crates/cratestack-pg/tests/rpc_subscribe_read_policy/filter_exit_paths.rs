//! Every write path that updates a row hands a filter-exit event its
//! pre-image: a single update, a batch update, a batch upsert and a single
//! upsert that updates, on a model that is `@@audit` as well as `@@subscribe`
//! (so the one pre-image read serves both), and that read takes no lock a
//! referencing table's insert would wait on.

use std::time::Duration;

use cratestack::serde_json::{Value as Json, json};
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{CratestackContext, SubscriptionDispatcher, SubscriptionOpen, Value};

use super::cratestack_schema::{self, Cratestack};
use super::support::pg;

fn caller(id: i64) -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::Int(id))])
}

#[tokio::test]
async fn every_update_path_sends_the_pre_image_to_a_filter_exit() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let _guard = pg::serial_guard().await;
    for statement in [
        "DROP TABLE IF EXISTS sub_auditeds",
        "CREATE TABLE sub_auditeds (id BIGINT PRIMARY KEY, body TEXT NOT NULL)",
        "DROP TABLE IF EXISTS cratestack_audit, cratestack_event_outbox",
    ] {
        cratestack::sqlx::query(statement)
            .execute(&test_pg.pool)
            .await
            .expect(statement);
    }
    // The shared pool is small, and the feed's listener holds one connection.
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&test_pg.url)
        .await
        .expect("a pool with room for the listener");
    let db = Cratestack::builder(pool).build();
    let ctx = caller(1);
    let mut feed = db
        .subscriptions()
        .open(
            SubscriptionOpen {
                op_id: "model.SubAudited.subscribe".to_owned(),
                input: json!({"filters": [{"key": "body__contains", "value": "keep"}]}),
                ctx: ctx.clone(),
            },
            cratestack::JsonCodec,
        )
        .await
        .expect("a filtered feed opens");
    let created = |id: i64, body: &str| cratestack_schema::CreateSubAuditedInput {
        id,
        body: body.to_owned(),
    };
    for id in 1..=4 {
        db.sub_audited()
            .create(created(id, &format!("keep {id}")))
            .run(&ctx)
            .await
            .expect("seeded");
    }

    let gone = |id: i64| cratestack_schema::UpdateSubAuditedInput {
        body: Some(format!("gone {id}")),
    };
    db.sub_audited()
        .update(1)
        .set(gone(1))
        .run(&ctx)
        .await
        .expect("single update");
    db.sub_audited()
        .batch_update(vec![(2, gone(2), None)])
        .run(&ctx)
        .await
        .expect("batch update");
    db.sub_audited()
        .batch_upsert(vec![created(3, "gone 3")])
        .run(&ctx)
        .await
        .expect("batch upsert");
    db.sub_audited()
        .upsert(created(4, "gone 4"))
        .run(&ctx)
        .await
        .expect("upsert");

    let mut exits = Vec::new();
    while let Ok(Some(Ok(event))) = tokio::time::timeout(
        Duration::from_millis(1000),
        cratestack::futures::StreamExt::next(&mut feed),
    )
    .await
    {
        let event = cratestack::serde_json::Value::from(event);
        if event["operation"] == "Updated" {
            exits.push((event["matches"].clone(), event["data"]["body"].clone()));
        }
    }
    let pre_images: Vec<(Json, Json)> = (1..=4)
        .map(|id| (json!(false), json!(format!("keep {id}"))))
        .collect();
    assert_eq!(
        exits, pre_images,
        "one exit per path, none of them the new body"
    );
    // The audit trail still got its before-snapshots from the same read.
    let audited: (i64,) = cratestack::sqlx::query_as(
        "SELECT count(*) FROM cratestack_audit WHERE operation = 'update' AND before IS NOT NULL",
    )
    .fetch_one(&test_pg.pool)
    .await
    .expect("audit rows");
    assert_eq!(audited.0, 4);
}

#[tokio::test]
async fn the_pre_image_read_does_not_block_a_referencing_insert() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let _guard = pg::serial_guard().await;
    for statement in [
        "DROP TABLE IF EXISTS sub_notes",
        "CREATE TABLE sub_notes (id BIGINT PRIMARY KEY, owner_id BIGINT NOT NULL, body TEXT NOT NULL, archived BOOLEAN NOT NULL)",
        "DROP TABLE IF EXISTS cratestack_event_outbox",
        "INSERT INTO sub_notes VALUES (1, 1, 'keep', false)",
    ] {
        cratestack::sqlx::query(statement)
            .execute(&test_pg.pool)
            .await
            .expect(statement);
    }
    let pool = PgPoolOptions::new()
        .max_connections(3)
        .connect(&test_pg.url)
        .await
        .expect("a pool for two sessions");
    let db = Cratestack::builder(pool.clone()).build();
    let mut tx = pool.begin().await.expect("the updating transaction");
    db.sub_note()
        .update(1)
        .set(cratestack_schema::UpdateSubNoteInput {
            ownerId: None,
            body: Some("changed".to_owned()),
            archived: None,
        })
        .run_in_tx(&mut tx, &caller(1))
        .await
        .expect("update inside the open transaction");
    // `FOR KEY SHARE` is the lock a foreign-key check on this row takes. A
    // `FOR UPDATE` pre-image read would still hold the row against it.
    let probe =
        cratestack::sqlx::query("SELECT 1 FROM sub_notes WHERE id = 1 FOR KEY SHARE NOWAIT")
            .execute(&pool)
            .await;
    tx.commit().await.expect("commit");
    probe.expect("a foreign-key check does not wait on the update");
}

/// Whether a foreign-key check on the row can still take its `FOR KEY SHARE`
/// while `update` is open on it; `None` when there is no database (a skip).
async fn key_share_is_free_after(update: cratestack_schema::UpdateSubKeyedInput) -> Option<bool> {
    let test_pg = pg::connect_or_skip().await?;
    for statement in [
        "DROP TABLE IF EXISTS sub_keyeds",
        "CREATE TABLE sub_keyeds (id BIGINT PRIMARY KEY, code TEXT NOT NULL UNIQUE, n BIGINT NOT NULL)",
        "DROP TABLE IF EXISTS cratestack_event_outbox",
        "INSERT INTO sub_keyeds VALUES (1, 'a', 1)",
    ] {
        cratestack::sqlx::query(statement)
            .execute(&test_pg.pool)
            .await
            .expect(statement);
    }
    let pool = PgPoolOptions::new()
        .max_connections(3)
        .connect(&test_pg.url)
        .await
        .expect("a pool for two sessions");
    let db = Cratestack::builder(pool.clone()).build();
    let mut tx = pool.begin().await.expect("the updating transaction");
    db.sub_keyed()
        .update(1)
        .set(update)
        .run_in_tx(&mut tx, &caller(1))
        .await
        .expect("update inside the open transaction");
    let probe =
        cratestack::sqlx::query("SELECT 1 FROM sub_keyeds WHERE id = 1 FOR KEY SHARE NOWAIT")
            .execute(&pool)
            .await;
    tx.commit().await.expect("commit");
    Some(probe.is_ok())
}

#[tokio::test]
async fn an_update_of_a_unique_column_holds_the_row_against_key_share() {
    let _guard = pg::serial_guard().await;
    let input = |code: Option<&str>, n: Option<i64>| cratestack_schema::UpdateSubKeyedInput {
        code: code.map(str::to_owned),
        n,
    };
    let Some(non_key_free) = key_share_is_free_after(input(None, Some(2))).await else {
        return;
    };
    assert!(non_key_free, "a non-key update leaves FOR KEY SHARE free");
    let key_free = key_share_is_free_after(input(Some("b"), Some(2))).await;
    assert_eq!(
        key_free,
        Some(false),
        "a unique-column update holds the row"
    );
}
