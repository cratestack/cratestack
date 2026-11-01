//! A model feed's input (`docs/design/subscriptions-websocket.md` §3.1,
//! §5.3, slice S3): `list`'s filter syntax plus `operations` and `fields`,
//! as the query string of the SSE endpoint and as the `RpcSubscribeInput` of
//! the transport-neutral dispatcher. The filter narrows what the read policy
//! already allows; it never widens it.

use std::time::Duration;

use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::serde_json::json;
use cratestack::{
    CratestackContext, CratestackError, SubscriptionDispatcher, SubscriptionOpen, Value,
};
use tower::util::ServiceExt;

use super::support::pg;
use super::{HeaderCaller, NoProcedures, cratestack_schema, delivered_ids, received};

fn caller(id: i64) -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::Int(id))])
}

async fn create(db: &cratestack_schema::Cratestack, id: i64, owner: i64, body: &str) {
    db.sub_note()
        .bind(caller(owner))
        .create(cratestack_schema::CreateSubNoteInput {
            id,
            ownerId: owner,
            body: body.to_owned(),
            archived: false,
        })
        .run()
        .await
        .expect("note created");
}

#[tokio::test]
async fn a_feed_filters_projects_and_picks_operations() {
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
    let request = Request::get(
        "/rpc/subscribe/model.SubNote.subscribe?body__contains=keep&operations=created&fields=id,body",
    )
    .header("accept", "text/event-stream")
    .header("x-caller", "1")
    .body(Body::empty())
    .unwrap();
    let sse = router.clone().oneshot(request).await.unwrap();
    assert_eq!(sse.status(), StatusCode::OK);
    // The same input through the dispatcher, as WebTransport sends it.
    let mut feed = db
        .subscriptions()
        .open(SubscriptionOpen {
            op_id: "model.SubNote.subscribe".to_owned(),
            input: json!({"filters": [{"key": "id", "value": "2"}]}),
            ctx: caller(1),
        })
        .await
        .expect("a filtered feed opens");

    create(&db, 1, 1, "keep this").await;
    create(&db, 2, 1, "drop this").await;
    create(&db, 3, 2, "keep, but owner 2's").await;

    let text = received(sse, Duration::from_millis(1000)).await;
    assert_eq!(delivered_ids(&text), [1], "SSE: {text}");
    assert!(
        text.contains(r#""data":{"body":"keep this","id":1}"#)
            || text.contains(r#""data":{"id":1,"body":"keep this"}"#),
        "fields projects the record to id and body: {text}"
    );
    assert!(!text.contains("ownerId"), "projected away: {text}");

    let event = tokio::time::timeout(
        Duration::from_secs(5),
        cratestack::futures::StreamExt::next(&mut feed),
    )
    .await
    .expect("an event within 5s")
    .expect("the feed does not end")
    .expect("an item");
    assert_eq!(event["data"]["id"], 2);
}

#[tokio::test]
async fn a_filter_a_feed_cannot_decide_is_refused_at_open() {
    let pool = cratestack::sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused@127.0.0.1:1/none")
        .expect("a lazy pool does not connect");
    let dispatch = cratestack_schema::Cratestack::builder(pool)
        .build()
        .subscriptions();
    for (query, why) in [
        ("limit=10", "paging means nothing for a stream"),
        ("operations=renamed", "an unknown operation"),
        ("fields=nope", "an unknown field"),
        ("nope=1", "an unknown filter"),
    ] {
        let refused = dispatch
            .open_query("model.SubNote.subscribe", Some(query.to_owned()), caller(1))
            .await;
        assert!(
            matches!(refused, Err(CratestackError::BadRequest(_))),
            "{query}: {why}"
        );
    }
}
