//! `IsolatedCratestack::topics()` (`docs/design/subscriptions-websocket.md`
//! §3.4): a publish from an `@isolation` procedure is delivered only if the
//! attempt commits, never for one that errors or is retried. With
//! `PgNotifyBackplane` it notifies inside the transaction; with
//! `InProcessBackplane`, which cannot, it is queued and sent after the commit.
//!
//! Postgres-backed: run it with `CRATESTACK_REQUIRE_DB=1`, or a missing
//! database is a skip that still prints `ok` (CLAUDE.md, "Critical test
//! gotcha").

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use cratestack::futures::StreamExt;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{
    CratestackContext, CratestackError, InProcessBackplane, SubscriptionStream, Value,
    include_server_schema,
};
use tokio::time::timeout;

include_server_schema!(
    "tests/fixtures/subscription_topics_isolated.cstack",
    db = Postgres
);

mod support;

use cratestack_schema::procedures::post;
use cratestack_schema::{ChatMessage, Cratestack, IsolatedCratestack, RoomArgs};
use support::pg;

fn message(room: &str, body: &str) -> ChatMessage {
    ChatMessage {
        roomId: room.to_owned(),
        body: body.to_owned(),
    }
}

/// Everything the stream delivers within a short window.
async fn drain(mut stream: SubscriptionStream<ChatMessage>) -> Vec<String> {
    let mut bodies = Vec::new();
    while let Ok(Some(item)) = timeout(Duration::from_millis(750), stream.next()).await {
        bodies.push(item.expect("not an error").body);
    }
    bodies
}

/// A pool with room for the backplane's listener and an attempt, so a broken
/// publish fails an assertion instead of starving the pool.
async fn pool(url: &str) -> cratestack::sqlx::PgPool {
    PgPoolOptions::new()
        .max_connections(4)
        .connect(url)
        .await
        .expect("pool")
}

/// Runs `post` for `room`, calling `body(handle, attempt)` in each attempt;
/// returns the outcome and how many times the body ran.
async fn run_post<F, Fut>(
    db: &Cratestack,
    room: &str,
    body: F,
) -> (Result<(), CratestackError>, usize)
where
    F: Fn(IsolatedCratestack, usize) -> Fut + Clone,
    Fut: Future<Output = Result<(), CratestackError>>,
{
    let args = post::Args {
        args: RoomArgs {
            roomId: room.to_owned(),
        },
    };
    let ctx = CratestackContext::authenticated([("id".to_owned(), Value::Int(1))]);
    let attempts = Arc::new(AtomicUsize::new(0));
    let runs = attempts.clone();
    let outcome = post::invoke_with_db(db, &args, &ctx, move |tx_db, _authorized| async move {
        body(tx_db, runs.fetch_add(1, Ordering::SeqCst) + 1).await
    })
    .await
    .map(drop);
    (outcome, attempts.load(Ordering::SeqCst))
}

/// Publishes `bodies` in order from the isolated handle.
async fn publish_all(
    tx_db: &IsolatedCratestack,
    room: &str,
    bodies: &[&str],
) -> Result<(), CratestackError> {
    for body in bodies {
        let topic = tx_db.topics().room_message(room);
        topic.publish(&message(room, body)).await?;
    }
    Ok(())
}

/// Commit, rollback and retry, on whichever backplane `db` has.
async fn commit_only(db: &Cratestack, room: &'static str) -> Vec<String> {
    let stream = db.topics().room_message(room).subscribe().await.unwrap();
    let (committed, _) = run_post(db, room, |tx_db, _| async move {
        publish_all(&tx_db, room, &["one", "two"]).await
    })
    .await;
    committed.expect("commits");
    let (failed, _) = run_post(db, room, |tx_db, _| async move {
        publish_all(&tx_db, room, &["rolled back"]).await?;
        Err(CratestackError::Conflict("no".to_owned()))
    })
    .await;
    assert!(matches!(failed, Err(CratestackError::Conflict(_))));
    let (retried, runs) = run_post(db, room, |tx_db, attempt| async move {
        publish_all(&tx_db, room, &["retried"]).await?;
        match attempt {
            1 => Err(CratestackError::Database(
                "could not serialize access".to_owned(),
            )),
            _ => Ok(()),
        }
    })
    .await;
    retried.expect("commits on the retry");
    assert_eq!(runs, 2);
    drain(stream).await
}

#[tokio::test]
async fn pg_notify_delivers_an_isolated_publish_only_when_the_attempt_commits() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let db = Cratestack::builder(pool(&test_pg.url).await).build();
    assert_eq!(commit_only(&db, "pg").await, ["one", "two", "retried"]);
}

#[tokio::test]
async fn in_process_delivers_an_isolated_publish_only_when_the_attempt_commits() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let db = Cratestack::builder(pool(&test_pg.url).await)
        .with_backplane(Arc::new(InProcessBackplane::new()))
        .build();
    assert_eq!(commit_only(&db, "memory").await, ["one", "two", "retried"]);
}

#[tokio::test]
async fn identical_isolated_publishes_are_each_delivered() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let db = Cratestack::builder(pool(&test_pg.url).await).build();
    let stream = db.topics().room_message("dup").subscribe().await.unwrap();
    let (outcome, _) = run_post(&db, "dup", |tx_db, _| async move {
        publish_all(&tx_db, "dup", &["same", "same"]).await
    })
    .await;
    outcome.expect("commits");
    // A plain handle publishes at once, and twice just the same.
    let plain = db.topics().room_message("dup");
    for _ in 0..2 {
        plain.publish(&message("dup", "same")).await.unwrap();
    }
    assert_eq!(drain(stream).await, ["same"; 4]);
}

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
