//! A REST schema serves its `@@subscribe` feed at `GET /<plural>/$subscribe`
//! (`docs/design/subscriptions-websocket.md` §7.2, slice S3): the same
//! dispatcher as RPC's `/rpc/subscribe/{op}`, so the same read policy, open
//! refusal and feed input (`list`'s filters, `operations`, `fields`).

use std::time::Duration;

use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::{CratestackContext, Value};
use tower::util::ServiceExt;

use super::support::pg;
use super::{HeaderCaller, delivered_ids, received};

mod schema {
    use cratestack::include_server_schema;
    include_server_schema!("tests/fixtures/subscribe_rest.cstack", db = Postgres);

    #[derive(Clone)]
    pub struct NoProcedures;

    impl cratestack_schema::procedures::ProcedureRegistry for NoProcedures {}
}

use schema::cratestack_schema;

async fn get(
    router: &cratestack::axum::Router,
    uri: &str,
    caller: Option<i64>,
) -> cratestack::axum::response::Response {
    let mut request = Request::get(uri).header("accept", "text/event-stream");
    if let Some(id) = caller {
        request = request.header("x-caller", id.to_string());
    }
    router
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .expect("subscribe route")
}

#[tokio::test]
async fn a_rest_schema_serves_its_feed_at_dollar_subscribe() {
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
        cratestack_codec_cbor::CborCodec,
        HeaderCaller,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    let route = cratestack::model_subscribe_route("SubNote");

    let owner = get(&router, &route, Some(1)).await;
    assert_eq!(owner.status(), StatusCode::OK);
    let filtered = get(&router, &format!("{route}?body__contains=keep"), Some(1)).await;
    assert_eq!(filtered.status(), StatusCode::OK);
    let anonymous = get(&router, &route, None).await;
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    // `list`'s paging has no meaning on a feed.
    let paged = get(&router, &format!("{route}?limit=5"), Some(1)).await;
    assert_eq!(paged.status(), StatusCode::BAD_REQUEST);

    for (id, owner_id, archived, body) in [
        (1, 1, false, "keep"),
        (2, 1, true, "keep"),
        (3, 2, false, "keep"),
        (4, 1, false, "drop"),
    ] {
        db.sub_note()
            .bind(CratestackContext::authenticated([(
                "id".to_owned(),
                Value::Int(owner_id),
            )]))
            .create(cratestack_schema::CreateSubNoteInput {
                id,
                ownerId: owner_id,
                body: body.to_owned(),
                archived,
            })
            .run()
            .await
            .expect("note created");
    }
    let window = Duration::from_millis(750);
    let (owner, filtered) = tokio::join!(received(owner, window), received(filtered, window));
    assert_eq!(delivered_ids(&owner), [1, 4], "owner: {owner}");
    assert_eq!(delivered_ids(&filtered), [1], "filtered: {filtered}");
}
