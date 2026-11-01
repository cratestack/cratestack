//! A `@@subscribe` SSE subscriber receives only the events whose record its
//! read policy lets it read, the rows `get` would return to it: none for an
//! anonymous caller, its own for an owner, and never an archived one (a
//! `@deny`). A read policy over a column the event does not carry
//! (`@server_only`) refuses the subscription instead of guessing.
//!
//! Postgres-backed: run it with `CRATESTACK_REQUIRE_DB=1`, or a missing
//! database is a skip that still prints `ok` (CLAUDE.md, "Critical test
//! gotcha").

use std::time::Duration;

use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::axum::response::Response;
use cratestack::futures::StreamExt;
use cratestack::include_server_schema;
use cratestack::{AuthProvider, CratestackContext, CratestackError, RequestContext, Value};
use cratestack_codec_cbor::CborCodec;
use tokio::time::timeout;
use tower::util::ServiceExt;

include_server_schema!("tests/fixtures/subscribe_read_policy.cstack", db = Postgres);

#[path = "rpc_subscribe_read_policy/dispatcher.rs"]
mod dispatcher;
#[path = "rpc_subscribe_read_policy/replicas.rs"]
mod replicas;
mod support;

use support::pg;

/// `x-caller: <id>` authenticates as that id; no header is anonymous.
#[derive(Clone)]
struct HeaderCaller;

impl AuthProvider for HeaderCaller {
    type Error = CratestackError;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        let caller = request
            .headers
            .get("x-caller")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<i64>().ok());
        core::future::ready(Ok(match caller {
            Some(id) => CratestackContext::authenticated([("id".to_owned(), Value::Int(id))]),
            None => CratestackContext::anonymous(),
        }))
    }
}

#[derive(Clone)]
struct NoProcedures;

impl cratestack_schema::procedures::ProcedureRegistry for NoProcedures {}

async fn subscribe(router: &cratestack::axum::Router, op: &str, caller: Option<i64>) -> Response {
    let mut request =
        Request::get(format!("/rpc/subscribe/{op}")).header("accept", "text/event-stream");
    if let Some(id) = caller {
        request = request.header("x-caller", id.to_string());
    }
    router
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .expect("subscribe dispatch")
}

/// Everything the stream sends within `window`; it never ends on its own.
async fn received(response: Response, window: Duration) -> String {
    let mut body = response.into_body().into_data_stream();
    let mut text = Vec::new();
    while let Ok(Some(chunk)) = timeout(window, body.next()).await {
        text.extend_from_slice(&chunk.expect("body chunk"));
    }
    String::from_utf8(text).expect("SSE is UTF-8")
}

fn delivered_ids(text: &str) -> Vec<i64> {
    text.split("\n\n")
        .filter_map(|event| event.strip_prefix("event: message\ndata: "))
        .map(|data| {
            let frame: cratestack::serde_json::Value =
                cratestack::serde_json::from_str(data).expect("frame is JSON");
            frame["next"]["data"]["id"]
                .as_i64()
                .expect("event record id")
        })
        .collect()
}

#[tokio::test]
async fn subscribers_receive_only_the_events_they_may_read() {
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
    let router = cratestack_schema::axum::rpc_router(
        db.clone(),
        NoProcedures,
        (),
        CborCodec,
        HeaderCaller,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );

    // Subscribed before any write: each subscription is live on the
    // backplane by the time its response is returned.
    let op = "model.SubNote.subscribe";
    let owner_one = subscribe(&router, op, Some(1)).await;
    let owner_two = subscribe(&router, op, Some(2)).await;
    for response in [&owner_one, &owner_two] {
        assert_eq!(response.status(), StatusCode::OK);
    }
    // No row passes `ownerId == auth().id` without an id: refused at open
    // (design §5.1) with a real 401, though the client accepts only SSE.
    let anonymous = subscribe(&router, op, None).await;
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

    for (id, owner, archived) in [(1, 1, false), (2, 1, true), (3, 2, false)] {
        let ctx = CratestackContext::authenticated([("id".to_owned(), Value::Int(owner))]);
        db.sub_note()
            .bind(ctx)
            .create(cratestack_schema::CreateSubNoteInput {
                id,
                ownerId: owner,
                body: format!("note {id}"),
                archived,
            })
            .run()
            .await
            .expect("note created");
    }

    let window = Duration::from_millis(750);
    let (owner_one, owner_two) =
        tokio::join!(received(owner_one, window), received(owner_two, window),);
    assert_eq!(delivered_ids(&owner_one), [1], "owner 1: {owner_one}");
    assert_eq!(delivered_ids(&owner_two), [3], "owner 2: {owner_two}");
}

#[tokio::test]
async fn a_policy_over_a_column_the_event_lacks_refuses_the_subscription() {
    // Decided before any query: the pool never connects.
    let pool = cratestack::sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused@127.0.0.1:1/none")
        .expect("a lazy pool does not connect");
    let router = cratestack_schema::axum::rpc_router(
        cratestack_schema::Cratestack::builder(pool).build(),
        NoProcedures,
        (),
        CborCodec,
        HeaderCaller,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    // An error body needs a codec the client accepts besides SSE, or it is
    // a 406 instead of the refusal itself.
    let request = Request::get("/rpc/subscribe/model.SubSecret.subscribe")
        .header("accept", "text/event-stream, application/cbor")
        .header("x-caller", "1")
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.expect("subscribe dispatch");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}
