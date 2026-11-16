//! A REST schema's `@@subscribe` feed over `application/cbor-seq`
//! (cratestack#1001): `GET /<plural>/$subscribe` with that `Accept` answers a
//! CBOR sequence of `ServerFrame`s — `ready`, then one `next` per event the
//! subscriber may read — instead of SSE. Same dispatcher, same policy.

use std::time::Duration;

use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode, header};
use cratestack::{CratestackContext, Value};
use cratestack_codec_cbor::{CborCodec, CborSeqChunkDecoder};
use cratestack_core::CratestackCodec;
use cratestack_core::subscription_wire::ServerFrame;
use futures_util::StreamExt;
use tower::util::ServiceExt;

use super::support::pg;
use super::{HeaderCaller, rest::schema};

use schema::cratestack_schema;

async fn open(
    router: &cratestack::axum::Router,
    accept: &str,
    caller: Option<i64>,
) -> cratestack::axum::response::Response {
    let route = cratestack::model_subscribe_route("SubNote");
    let mut request = Request::get(route).header("accept", accept);
    if let Some(id) = caller {
        request = request.header("x-caller", id.to_string());
    }
    router
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .expect("subscribe route")
}

/// Every frame the stream sends within `window`; it never ends on its own.
async fn frames(
    response: cratestack::axum::response::Response,
    window: Duration,
) -> Vec<ServerFrame> {
    let mut body = response.into_body().into_data_stream();
    let mut decoder = CborSeqChunkDecoder::new();
    let mut frames = Vec::new();
    while let Ok(Some(chunk)) = tokio::time::timeout(window, body.next()).await {
        for item in decoder.feed_chunk(&chunk.expect("body chunk")).unwrap() {
            frames.push(CborCodec.decode(&item).expect("a server frame"));
        }
    }
    assert_eq!(decoder.pending_len(), 0, "no partial frame");
    frames
}

#[tokio::test]
async fn a_rest_model_feed_streams_ready_then_events_as_cbor_frames() {
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
    let router = cratestack_schema::axum::router(
        db.clone(),
        schema::NoProcedures,
        (),
        CborCodec,
        HeaderCaller,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );

    let owner = open(&router, "application/cbor-seq", Some(1)).await;
    assert_eq!(owner.status(), StatusCode::OK);
    assert_eq!(
        owner.headers()[header::CONTENT_TYPE],
        "application/cbor-seq"
    );
    let anonymous = open(&router, "application/cbor-seq", None).await;
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        anonymous.headers()[header::CONTENT_TYPE],
        "application/json"
    );
    let bad = open(&router, "application/cbor", Some(1)).await;
    assert_eq!(bad.status(), StatusCode::NOT_ACCEPTABLE);

    for (id, owner_id) in [(1, 1), (2, 2)] {
        db.sub_note()
            .bind(CratestackContext::authenticated([(
                "id".to_owned(),
                Value::Int(owner_id),
            )]))
            .create(cratestack_schema::CreateSubNoteInput {
                id,
                ownerId: owner_id,
                body: "keep".to_owned(),
                archived: false,
            })
            .run()
            .await
            .expect("note created");
    }
    let frames = frames(owner, Duration::from_millis(750)).await;
    assert_eq!(frames.len(), 2, "ready, then the one note it may read");
    assert!(frames[0].ready.is_some());
    assert_eq!(frames[0].id, 1);
    assert_eq!(frames[1].id, 2);
    let event = serde_json::Value::from(frames[1].next.clone().expect("a `next` frame"));
    assert_eq!(event["data"]["id"], 1, "{event}");
}
