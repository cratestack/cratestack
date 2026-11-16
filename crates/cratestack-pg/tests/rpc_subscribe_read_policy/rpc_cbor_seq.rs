//! An RPC model feed over `application/cbor-seq` (cratestack#1001): `GET` and
//! `POST /rpc/subscribe/model.SubNote.subscribe` answer `ready`, then a `next`
//! frame per readable event, the `POST` taking its input as CBOR.

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
use super::{HeaderCaller, NoProcedures, cratestack_schema};

const OP: &str = "/rpc/subscribe/model.SubNote.subscribe";

fn request(post: bool) -> Request<Body> {
    let builder = if post {
        Request::post(OP)
    } else {
        Request::get(OP)
    };
    let builder = builder
        .header("accept", "application/cbor-seq")
        .header("x-caller", "1");
    if post {
        let body = CborCodec.encode(&serde_json::json!({})).unwrap();
        builder
            .header("content-type", "application/cbor")
            .body(Body::from(body))
            .unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    }
}

async fn frames(response: cratestack::axum::response::Response) -> Vec<ServerFrame> {
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "application/cbor-seq"
    );
    let mut body = response.into_body().into_data_stream();
    let mut decoder = CborSeqChunkDecoder::new();
    let mut frames = Vec::new();
    while let Ok(Some(chunk)) = tokio::time::timeout(Duration::from_millis(750), body.next()).await
    {
        for item in decoder.feed_chunk(&chunk.expect("body chunk")).unwrap() {
            frames.push(CborCodec.decode(&item).expect("a server frame"));
        }
    }
    assert_eq!(decoder.pending_len(), 0, "no partial frame");
    frames
}

#[tokio::test]
async fn get_and_post_stream_ready_then_the_readable_events() {
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
        CborCodec,
        HeaderCaller,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    let got = router.clone().oneshot(request(false)).await.unwrap();
    let posted = router.clone().oneshot(request(true)).await.unwrap();

    for (id, owner) in [(1, 1), (2, 2)] {
        let ctx = CratestackContext::authenticated([("id".to_owned(), Value::Int(owner))]);
        db.sub_note()
            .bind(ctx)
            .create(cratestack_schema::CreateSubNoteInput {
                id,
                ownerId: owner,
                body: format!("note {id}"),
                archived: false,
            })
            .run()
            .await
            .expect("note created");
    }
    let (got, posted) = tokio::join!(frames(got), frames(posted));
    for (via, frames) in [("GET", got), ("POST", posted)] {
        assert_eq!(frames.len(), 2, "{via}: ready, then the one readable note");
        assert!(frames[0].ready.is_some(), "{via}");
        let event = serde_json::Value::from(frames[1].next.clone().expect("a `next` frame"));
        assert_eq!(event["data"]["id"], 1, "{via}: {event}");
    }
}
