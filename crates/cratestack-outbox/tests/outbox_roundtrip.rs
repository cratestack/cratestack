//! Integration tests against a real Postgres. Skips (rather than failing)
//! when no database backend is configured — see `tests/support/pg.rs`.
//! Run via `just pg-up && CRATESTACK_TEST_DATABASE_URL=... cargo test -p
//! cratestack-outbox`, or `CRATESTACK_USE_TESTCONTAINERS=1 cargo test -p
//! cratestack-outbox` for an ephemeral per-run container.
//!
//! Covers the two invariants the outbox pattern exists to provide:
//! - `persist_in_tx` genuinely participates in the caller's transaction —
//!   it commits and rolls back with it, not independently.
//! - `drain` orders strictly by UUIDv7 `id`, so a snapshotter's cursor
//!   (`after_id`) never skips or repeats a row.

mod support;

use cratestack_outbox::{DrainRequest, NewEvent, OUTBOX_EVENTS_DDL, OutboxClient};
use cratestack_sqlx::sqlx::{Executor, Row};
use support::pg;

async fn reset(pool: &cratestack_sqlx::sqlx::PgPool) {
    pool.execute("DROP TABLE IF EXISTS cratestack_outbox_events, outbox_it_business")
        .await
        .expect("drop");
    pool.execute(OUTBOX_EVENTS_DDL).await.expect("install ddl");
    pool.execute("CREATE TABLE outbox_it_business (id TEXT PRIMARY KEY)")
        .await
        .expect("create business table");
}

#[tokio::test]
async fn persist_in_tx_rolls_back_with_caller_transaction() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    reset(&pool).await;
    let outbox = OutboxClient::from_pool(pool.clone());

    let mut tx = pool.begin().await.expect("begin");
    cratestack_sqlx::sqlx::query("INSERT INTO outbox_it_business (id) VALUES ($1)")
        .bind("biz_1")
        .execute(&mut *tx)
        .await
        .expect("insert business row");
    outbox
        .persist_in_tx(
            &mut tx,
            NewEvent::new(
                "business",
                "biz_1",
                "business.created",
                serde_json::json!({"ok": true}),
            ),
        )
        .await
        .expect("persist_in_tx");
    tx.rollback().await.expect("rollback");

    let business_count: i64 =
        cratestack_sqlx::sqlx::query("SELECT count(*) AS c FROM outbox_it_business")
            .fetch_one(&pool)
            .await
            .expect("count business")
            .try_get("c")
            .expect("read count");
    assert_eq!(
        business_count, 0,
        "business row must not survive the rollback"
    );

    let drained = outbox
        .drain(&DrainRequest {
            after_id: None,
            max: 10,
        })
        .await
        .expect("drain");
    assert!(
        drained.events.is_empty(),
        "outbox event must not survive the rollback of the transaction it was written in"
    );
}

#[tokio::test]
async fn persist_in_tx_commits_atomically_with_caller_transaction() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    reset(&pool).await;
    let outbox = OutboxClient::from_pool(pool.clone());

    let mut tx = pool.begin().await.expect("begin");
    cratestack_sqlx::sqlx::query("INSERT INTO outbox_it_business (id) VALUES ($1)")
        .bind("biz_2")
        .execute(&mut *tx)
        .await
        .expect("insert business row");
    let event_id = outbox
        .persist_in_tx(
            &mut tx,
            NewEvent::new(
                "business",
                "biz_2",
                "business.created",
                serde_json::json!({"ok": true}),
            ),
        )
        .await
        .expect("persist_in_tx");
    tx.commit().await.expect("commit");

    let business_count: i64 =
        cratestack_sqlx::sqlx::query("SELECT count(*) AS c FROM outbox_it_business")
            .fetch_one(&pool)
            .await
            .expect("count business")
            .try_get("c")
            .expect("read count");
    assert_eq!(business_count, 1, "business row must survive the commit");

    let drained = outbox
        .drain(&DrainRequest {
            after_id: None,
            max: 10,
        })
        .await
        .expect("drain");
    assert_eq!(drained.events.len(), 1);
    assert_eq!(drained.events[0].id, event_id);
}

#[tokio::test]
async fn drain_orders_strictly_by_uuidv7_id_and_pages_via_cursor() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    reset(&pool).await;
    let outbox = OutboxClient::from_pool(pool.clone());

    let mut ids: Vec<String> = Vec::new();
    for (aggregate_id, event_type) in [
        ("rev_1", "review.created"),
        ("rev_2", "review.approved"),
        ("rev_3", "review.approved"),
    ] {
        let id = outbox
            .persist(NewEvent::new(
                "review",
                aggregate_id,
                event_type,
                serde_json::json!({"aggregate_id": aggregate_id}),
            ))
            .await
            .expect("persist");
        ids.push(id);
        // UUIDv7 has microsecond resolution; a 1ms sleep guarantees
        // lexical monotonicity even on a host whose clock could otherwise
        // return identical values back-to-back.
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    assert!(
        ids.windows(2).all(|pair| pair[0] < pair[1]),
        "ids must be strictly increasing: {ids:?}"
    );

    let page_1 = outbox
        .drain(&DrainRequest {
            after_id: None,
            max: 2,
        })
        .await
        .expect("drain page 1");
    assert_eq!(page_1.events.len(), 2);
    assert_eq!(page_1.events[0].aggregate_id, "rev_1");
    assert_eq!(page_1.events[1].aggregate_id, "rev_2");
    assert_eq!(
        page_1.next_cursor.as_deref(),
        Some(page_1.events[1].id.as_str())
    );

    let page_2 = outbox
        .drain(&DrainRequest {
            after_id: page_1.next_cursor,
            max: 10,
        })
        .await
        .expect("drain page 2");
    assert_eq!(page_2.events.len(), 1);
    assert_eq!(page_2.events[0].aggregate_id, "rev_3");
    assert!(
        page_2.events[0].id > page_1.events[1].id,
        "page 2's row must sort after every row already drained"
    );
}

#[tokio::test]
async fn drain_clamps_an_excessive_max_to_the_hard_ceiling() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    reset(&pool).await;
    let outbox = OutboxClient::from_pool(pool.clone());

    for i in 0..5 {
        outbox
            .persist(NewEvent::new(
                "review",
                format!("rev_{i}"),
                "review.created",
                serde_json::json!({}),
            ))
            .await
            .expect("persist");
    }

    let response = outbox
        .drain(&DrainRequest {
            after_id: None,
            max: i64::MAX,
        })
        .await
        .expect("drain with huge max must clamp, not error");
    assert_eq!(response.events.len(), 5);
}

#[tokio::test]
async fn gc_older_than_sweeps_everything_before_the_cutoff() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    reset(&pool).await;
    let outbox = OutboxClient::from_pool(pool.clone());

    for i in 0..3 {
        outbox
            .persist(NewEvent::new(
                "review",
                format!("rev_{i}"),
                "review.created",
                serde_json::json!({}),
            ))
            .await
            .expect("persist");
    }

    let removed = outbox
        .gc_older_than(chrono::Utc::now() + chrono::Duration::seconds(1))
        .await
        .expect("gc");
    assert_eq!(removed, 3);

    let after = outbox
        .drain(&DrainRequest {
            after_id: None,
            max: 10,
        })
        .await
        .expect("drain after gc");
    assert!(after.events.is_empty());
}
