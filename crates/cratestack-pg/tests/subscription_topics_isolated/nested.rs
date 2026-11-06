//! A topic inside `db.transaction(..)` on the isolated handle: the
//! transaction holds the handle, so the call is `publish_in_tx(tx, ..)`,
//! and a message published in a `transaction(..)` that rolls back is never
//! delivered, even though the attempt around it commits.

use std::sync::Arc;

use cratestack::{CratestackError, InProcessBackplane};

use super::{Cratestack, drain, message, pg, pool, run_post};

#[tokio::test]
async fn publish_in_tx_inside_the_handles_transaction_rides_the_attempt() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let db = Cratestack::builder(pool(&test_pg.url).await).build();
    let stream = db.topics().room_message("tx").subscribe().await.unwrap();
    let (outcome, _) = run_post(&db, "tx", |tx_db, _| async move {
        let topic = tx_db.topics().room_message("tx");
        tx_db
            .transaction(async |tx| {
                // `publish` would need the transaction this closure holds.
                let refused = topic.publish(&message("tx", "refused")).await;
                assert!(refused.unwrap_err().to_string().contains("publish_in_tx"));
                topic.publish_in_tx(tx, &message("tx", "in tx")).await
            })
            .await
    })
    .await;
    outcome.expect("commits");
    assert_eq!(drain(stream).await, ["in tx"]);
}

/// One `transaction(..)` commits, the next rolls back; the attempt commits.
async fn kept_and_rolled_back(db: &Cratestack, room: &'static str) -> Vec<String> {
    let stream = db.topics().room_message(room).subscribe().await.unwrap();
    let (outcome, _) = run_post(db, room, |tx_db, _| async move {
        let topic = tx_db.topics().room_message(room);
        tx_db
            .transaction(async |tx| topic.publish_in_tx(tx, &message(room, "kept")).await)
            .await?;
        let rolled_back = tx_db
            .transaction(async |tx| {
                topic
                    .publish_in_tx(tx, &message(room, "rolled back"))
                    .await?;
                Err::<(), _>(CratestackError::Conflict("undo".to_owned()))
            })
            .await;
        assert!(matches!(rolled_back, Err(CratestackError::Conflict(_))));
        Ok(())
    })
    .await;
    outcome.expect("the attempt commits");
    drain(stream).await
}

#[tokio::test]
async fn pg_notify_drops_a_publish_whose_transaction_rolls_back() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let db = Cratestack::builder(pool(&test_pg.url).await).build();
    assert_eq!(kept_and_rolled_back(&db, "pg-tx").await, ["kept"]);
}

#[tokio::test]
async fn in_process_drops_a_publish_whose_transaction_rolls_back() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let db = Cratestack::builder(pool(&test_pg.url).await)
        .with_backplane(Arc::new(InProcessBackplane::new()))
        .build();
    assert_eq!(kept_and_rolled_back(&db, "memory-tx").await, ["kept"]);
}
