//! The generated `subscriptions::SubscriptionDispatch`
//! (`docs/design/subscriptions-websocket.md` §6.1, slice S5a), driven the
//! way every binding drives it: `SubscriptionDispatcher::open` with an op
//! id, an input and an authenticated context, then the stream. The SSE
//! endpoint is one adapter over it (`rpc_subscribe_read_policy.rs`,
//! `rpc_subscribe_sse.rs`), the WebTransport server another
//! (`webtransport_model_feed.rs`).
//!
//! A module of the `rpc_subscribe_read_policy` binary, which compiles the same
//! fixture, rather than a test binary of its own. Postgres-backed: run it with
//! `CRATESTACK_REQUIRE_DB=1`, or a missing database is a skip that still prints
//! `ok` (CLAUDE.md, "Critical test gotcha").

use std::time::Duration;

use cratestack::futures::StreamExt;
use cratestack::serde_json::{Value as Json, json};
use cratestack::{
    CratestackContext, CratestackError, SubscriptionDispatcher, SubscriptionOpen, Value,
};
use tokio::time::timeout;

use super::cratestack_schema;
use super::support::pg;

fn caller(id: i64) -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::Int(id))])
}

fn open(op_id: &str, input: Json, ctx: CratestackContext) -> SubscriptionOpen {
    SubscriptionOpen {
        op_id: op_id.to_owned(),
        input,
        ctx,
    }
}

fn lazy_db() -> cratestack_schema::Cratestack {
    let pool = cratestack::sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused@127.0.0.1:1/none")
        .expect("a lazy pool does not connect");
    cratestack_schema::Cratestack::builder(pool).build()
}

#[tokio::test]
async fn an_owner_receives_its_own_readable_records_as_json() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    // The tests of this binary reset the same tables.
    let _guard = pg::serial_guard().await;
    let pool = &test_pg.pool;
    for statement in [
        "DROP TABLE IF EXISTS sub_notes",
        "CREATE TABLE sub_notes (id BIGINT PRIMARY KEY, owner_id BIGINT NOT NULL, body TEXT NOT NULL, archived BOOLEAN NOT NULL)",
        "DROP TABLE IF EXISTS cratestack_event_outbox",
    ] {
        cratestack::sqlx::query(statement)
            .execute(pool)
            .await
            .expect(statement);
    }
    let db = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let dispatch = db.subscriptions();
    let mut owner = dispatch
        .open(open("model.SubNote.subscribe", Json::Null, caller(1)))
        .await
        .expect("an owner may subscribe");
    // No row passes `ownerId == auth().id` without an id: refused at open.
    // (An empty object is the same as no input.)
    let refused = dispatch
        .open(open(
            "model.SubNote.subscribe",
            json!({}),
            CratestackContext::anonymous(),
        ))
        .await;
    assert!(
        matches!(refused, Err(CratestackError::Unauthorized(_))),
        "anonymous is refused at open",
    );

    for (id, owner_id, archived) in [(1, 1, false), (2, 1, true), (3, 2, false)] {
        db.sub_note()
            .bind(caller(owner_id))
            .create(cratestack_schema::CreateSubNoteInput {
                id,
                ownerId: owner_id,
                body: format!("note {id}"),
                archived,
            })
            .run()
            .await
            .expect("note created");
    }

    let event = timeout(Duration::from_secs(5), owner.next())
        .await
        .expect("an event within 5s")
        .expect("the feed does not end")
        .expect("an item, not an error");
    assert_eq!(event["model"], "SubNote");
    assert_eq!(event["operation"], "Created");
    assert_eq!(event["data"]["id"], 1);
    assert_eq!(event["data"]["body"], "note 1");
    // Note 2 is archived (`@@deny`), note 3 is owner 2's.
    assert!(
        timeout(Duration::from_millis(500), owner.next())
            .await
            .is_err(),
        "no other note is readable by owner 1",
    );
}

#[tokio::test]
async fn refusals_happen_at_open() {
    let dispatch = lazy_db().subscriptions();
    let refused = |result: Result<_, CratestackError>| match result {
        Ok(_) => panic!("expected a refusal at open"),
        Err(error) => error,
    };

    let error = refused(
        dispatch
            .open(open("model.Nope.subscribe", Json::Null, caller(1)))
            .await,
    );
    assert!(matches!(error, CratestackError::NotFound(_)), "{error:?}");

    let error = refused(
        dispatch
            .open(open(
                "model.SubNote.subscribe",
                json!({"where": {}}),
                caller(1),
            ))
            .await,
    );
    assert!(matches!(error, CratestackError::BadRequest(_)), "{error:?}");

    // A read policy over a `@server_only` column cannot be decided on an
    // event, so the feed is refused rather than decided on a missing value.
    let error = refused(
        dispatch
            .open(open("model.SubSecret.subscribe", Json::Null, caller(1)))
            .await,
    );
    assert!(matches!(error, CratestackError::Internal(_)), "{error:?}");
}
