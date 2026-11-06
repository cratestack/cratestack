use std::time::Duration;

use super::{OutboxColumn, upgrade_outbox_table};
use crate::sqlx::{self, postgres::PgPoolOptions};
use crate::tests_bigint::pg_support::connect_or_skip;

#[test]
fn a_second_attempt_waits_for_the_first_and_for_the_retry_delay() {
    let column = OutboxColumn::default();
    let first = column.attempt().expect("the first attempt is free");
    assert!(column.attempt().is_none(), "one at a time");
    drop(first);
    assert!(column.attempt().is_none(), "not again within the delay");
}

/// Both upgraders see the old table, queue their `ALTER` behind a reader, and
/// run one after the other once it lets go: the second finds the column there.
/// Without `IF NOT EXISTS` it fails with 42701.
#[tokio::test]
async fn two_upgrades_at_once_both_succeed() {
    let Some(pg) = connect_or_skip().await else {
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with((*pg.pool.connect_options()).clone())
        .await
        .expect("a pool for the reader and two upgraders");
    for _ in 0..5 {
        for statement in [
            "DROP TABLE IF EXISTS cratestack_event_outbox",
            "CREATE TABLE cratestack_event_outbox (event_id UUID PRIMARY KEY, model TEXT NOT NULL, operation TEXT NOT NULL, occurred_at TIMESTAMPTZ NOT NULL, payload JSONB NOT NULL, delivered_at TIMESTAMPTZ, attempts BIGINT NOT NULL DEFAULT 0, last_error TEXT)",
        ] {
            sqlx::query(statement)
                .execute(&pool)
                .await
                .expect(statement);
        }
        let mut reader = pool.begin().await.expect("reader");
        sqlx::query("SELECT 1 FROM cratestack_event_outbox")
            .execute(&mut *reader)
            .await
            .expect("an ACCESS SHARE lock");
        let upgrades =
            async { tokio::join!(upgrade_outbox_table(&pool), upgrade_outbox_table(&pool)) };
        let release = async {
            tokio::time::sleep(Duration::from_millis(30)).await;
            reader.commit().await.expect("release");
        };
        let ((one, two), ()) = tokio::join!(upgrades, release);
        // A loaded machine can run an `ALTER` past its 100 ms lock timeout; that
        // is the one failure allowed. Losing the race (42701) is not.
        for upgrade in [one, two] {
            match upgrade {
                Ok(present) => assert!(present, "the table has the column"),
                Err(error) => assert_eq!(error.db_sqlstate(), Some("55P03"), "{error}"),
            }
        }
    }
}
