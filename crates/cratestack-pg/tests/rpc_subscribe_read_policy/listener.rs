//! Dropping a `PgNotifyBackplane` closes its listener connection even while
//! no notification ever arrives: an idle listener used to wait in `try_recv`
//! forever, leaking one connection per dropped backplane.

use std::time::Duration;

use cratestack::{PgNotifyBackplane, SubscriptionBackplane};

use super::support::pg;

async fn listeners(pool: &cratestack::sqlx::PgPool) -> i64 {
    cratestack::sqlx::query_scalar(
        "SELECT count(*) FROM pg_stat_activity WHERE query = 'LISTEN \"cratestack_events\"'",
    )
    .fetch_one(pool)
    .await
    .expect("pg_stat_activity readable")
}

async fn settles_at(pool: &cratestack::sqlx::PgPool, expected: i64) -> i64 {
    let mut seen = listeners(pool).await;
    for _ in 0..50 {
        if seen == expected {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        seen = listeners(pool).await;
    }
    seen
}

#[tokio::test]
async fn dropping_an_idle_backplane_closes_its_listener() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let _guard = pg::serial_guard().await;
    let before = listeners(&test_pg.pool).await;

    let backplane = PgNotifyBackplane::new(test_pg.pool.clone());
    let receiver = backplane
        .subscribe("cratestack.test.idle")
        .await
        .expect("subscribe starts the listener");
    assert_eq!(settles_at(&test_pg.pool, before + 1).await, before + 1);

    drop(receiver);
    drop(backplane);
    assert_eq!(
        settles_at(&test_pg.pool, before).await,
        before,
        "the listener connection outlived its backplane"
    );
}
