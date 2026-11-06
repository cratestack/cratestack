//! `IsolatedCratestack::topics()` (`docs/design/subscriptions-websocket.md`
//! §3.4): a publish from an `@isolation` procedure rides the procedure's
//! transaction, so it is delivered when the attempt commits and never for
//! one that errors or is retried.
//!
//! Postgres-backed (the topic rides `PgNotifyBackplane`): run it with
//! `CRATESTACK_REQUIRE_DB=1`, or a missing database is a skip that still
//! prints `ok` (CLAUDE.md, "Critical test gotcha").

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use cratestack::futures::StreamExt;
use cratestack::{
    CratestackContext, CratestackError, SubscriptionStream, Value, include_server_schema,
};
use tokio::time::timeout;

include_server_schema!(
    "tests/fixtures/subscription_topics_isolated.cstack",
    db = Postgres
);

mod support;

use cratestack_schema::procedures::post;
use cratestack_schema::{ChatMessage, RoomArgs};
use support::pg;

fn message(body: &str) -> ChatMessage {
    ChatMessage {
        roomId: "r1".to_owned(),
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

/// Runs `post`, publishing `bodies` in order from the isolated handle, then
/// `ending(attempt)`; returns the outcome and how many times the body ran.
async fn run_post(
    db: &cratestack_schema::Cratestack,
    bodies: &'static [&'static str],
    ending: fn(usize) -> Result<(), CratestackError>,
) -> (Result<ChatMessage, CratestackError>, usize) {
    let args = post::Args {
        args: RoomArgs {
            roomId: "r1".to_owned(),
        },
    };
    let ctx = CratestackContext::authenticated([("id".to_owned(), Value::Int(1))]);
    let attempts = Arc::new(AtomicUsize::new(0));
    let runs = attempts.clone();
    let outcome = post::invoke_with_db(db, &args, &ctx, move |tx_db, _authorized| async move {
        let attempt = runs.fetch_add(1, Ordering::SeqCst) + 1;
        for body in bodies {
            tx_db
                .topics()
                .room_message("r1")
                .publish(&message(body))
                .await?;
        }
        ending(attempt)?;
        Ok(message("returned"))
    })
    .await;
    (outcome, attempts.load(Ordering::SeqCst))
}

/// A serialization failure on the first attempt only.
fn conflict_once(attempt: usize) -> Result<(), CratestackError> {
    match attempt {
        1 => Err(CratestackError::Database(
            "could not serialize access".to_owned(),
        )),
        _ => Ok(()),
    }
}

#[tokio::test]
async fn an_isolated_publish_is_delivered_only_by_the_attempt_that_commits() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let db = cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build();
    let stream = db.topics().room_message("r1").subscribe().await.unwrap();

    let (committed, _) = run_post(&db, &["one", "two"], |_| Ok(())).await;
    committed.expect("commits");
    let (failed, _) = run_post(&db, &["rolled back"], |_| {
        Err(CratestackError::Conflict("no".to_owned()))
    })
    .await;
    assert!(matches!(failed, Err(CratestackError::Conflict(_))));
    let (retried, runs) = run_post(&db, &["retried"], conflict_once).await;
    retried.expect("commits on the retry");
    assert_eq!(runs, 2);

    assert_eq!(drain(stream).await, ["one", "two", "retried"]);
}
