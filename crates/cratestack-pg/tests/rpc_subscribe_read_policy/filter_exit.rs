//! Filter-exit events (`docs/design/subscriptions-websocket.md` §5.3): an
//! update that moves a row out of a subscriber's filter or read policy sends
//! it `matches: false` with the row as it last held it, never the new values;
//! a subscriber that held neither image hears nothing.

use std::time::Duration;

use cratestack::axum::http::StatusCode;
use cratestack::serde_json::{self, Value as Json, json};
use cratestack::{CratestackContext, SubscriptionDispatcher, SubscriptionOpen, Value};

use super::support::pg;
use super::{HeaderCaller, NoProcedures, cratestack_schema, received, subscribe};

fn caller(id: i64) -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::Int(id))])
}

async fn create(db: &cratestack_schema::Cratestack, id: i64, body: &str) {
    db.sub_note()
        .bind(caller(1))
        .create(cratestack_schema::CreateSubNoteInput {
            id,
            ownerId: 1,
            body: body.to_owned(),
            archived: false,
        })
        .run()
        .await
        .expect("note created");
}

async fn update(db: &cratestack_schema::Cratestack, id: i64, body: &str, archived: bool) {
    db.sub_note()
        .bind(caller(1))
        .update(id)
        .set(cratestack_schema::UpdateSubNoteInput {
            ownerId: None,
            body: Some(body.to_owned()),
            archived: Some(archived),
        })
        .run()
        .await
        .expect("note updated");
}

/// The `Updated` events a feed delivered: `(id, matches, body, archived)`.
fn updates(text: &str) -> Vec<(i64, bool, String, bool)> {
    text.split("\n\n")
        .filter_map(|event| event.strip_prefix("event: message\ndata: "))
        .map(|data| serde_json::from_str::<Json>(data).expect("frame is JSON")["next"].clone())
        .filter(|event| event["operation"] == "Updated")
        .map(|event| {
            let note = &event["data"];
            (
                note["id"].as_i64().unwrap(),
                event["matches"].as_bool().unwrap_or(true),
                note["body"].as_str().unwrap().to_owned(),
                note["archived"].as_bool().unwrap(),
            )
        })
        .collect()
}

#[tokio::test]
async fn an_update_out_of_a_filter_or_the_read_policy_is_a_filter_exit() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let _guard = pg::serial_guard().await;
    for statement in [
        "DROP TABLE IF EXISTS sub_notes",
        "CREATE TABLE sub_notes (id BIGINT PRIMARY KEY, owner_id BIGINT NOT NULL, body TEXT NOT NULL, archived BOOLEAN NOT NULL)",
        "DROP TABLE IF EXISTS cratestack_event_outbox",
    ] {
        cratestack::sqlx::query(statement)
            .execute(&test_pg.pool)
            .await
            .expect(statement);
    }
    let db = cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build();
    let router = cratestack_schema::axum::rpc_router(
        db.clone(),
        NoProcedures,
        (),
        cratestack_codec_cbor::CborCodec,
        HeaderCaller,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    let op = "model.SubNote.subscribe";
    // Owner 1 filtering on `keep`, owner 1 unfiltered, and a stranger.
    let filtered = subscribe(&router, &format!("{op}?body__contains=keep"), Some(1)).await;
    let unfiltered = subscribe(&router, op, Some(1)).await;
    let stranger = subscribe(&router, op, Some(2)).await;
    // The same filter through the dispatcher, as WebTransport and WebSocket send it.
    let mut dispatched = db
        .subscriptions()
        .open(
            SubscriptionOpen {
                op_id: op.to_owned(),
                input: json!({"filters": [{"key": "body__contains", "value": "keep"}]}),
                ctx: caller(1),
            },
            cratestack::JsonCodec,
        )
        .await
        .expect("a filtered feed opens");

    create(&db, 1, "keep me").await;
    create(&db, 2, "plain").await;
    create(&db, 3, "keep too").await;
    // 1 leaves the filter, still readable.
    update(&db, 1, "drop me", false).await;
    // 2 never matched, before or after.
    update(&db, 2, "plain still", false).await;
    // 3 stays in the filter but is archived: out of the read policy.
    update(&db, 3, "keep too", true).await;

    // Refused feeds would pass the silence below vacuously.
    for feed in [&filtered, &unfiltered, &stranger] {
        assert_eq!(feed.status(), StatusCode::OK);
    }
    let window = Duration::from_millis(1000);
    let (filtered, unfiltered, stranger) = tokio::join!(
        received(filtered, window),
        received(unfiltered, window),
        received(stranger, window),
    );
    // Exits carry the pre-image: "keep me", not "drop me"; archived false.
    assert_eq!(
        updates(&filtered),
        [
            (1, false, "keep me".to_owned(), false),
            (3, false, "keep too".to_owned(), false)
        ],
        "{filtered}"
    );
    // No filter: 1 and 2 are ordinary updates, 3 left the read policy.
    assert_eq!(
        updates(&unfiltered),
        [
            (1, true, "drop me".to_owned(), false),
            (2, true, "plain still".to_owned(), false),
            (3, false, "keep too".to_owned(), false)
        ],
        "{unfiltered}"
    );
    assert!(
        !filtered.contains(r#""archived":true"#) && !unfiltered.contains(r#""archived":true"#),
        "the archived post-image never leaves the server"
    );
    assert!(
        !stranger.contains("event: message"),
        "a caller who may not read the row hears nothing: {stranger}"
    );

    // The dispatcher: two creates, then the exit of note 1.
    let mut exit = None;
    while let Some(Ok(event)) = tokio::time::timeout(
        window,
        cratestack::futures::StreamExt::next(&mut dispatched),
    )
    .await
    .ok()
    .flatten()
    {
        let event = cratestack::serde_json::Value::from(event);
        if event["operation"] == "Updated" && exit.is_none() {
            exit = Some(event);
        }
    }
    let exit = exit.expect("the dispatcher delivers the exit");
    assert_eq!(exit["matches"], false);
    assert_eq!(exit["data"]["body"], "keep me");
}
