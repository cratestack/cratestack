//! A schema declaring `extension webtransport { }` serves its `@@subscribe`
//! model feeds over WebTransport through the generated dispatcher
//! (`db.subscriptions().webtransport(config, auth)`, slice S5a), with the
//! same per-event read policy as the SSE endpoint: an owner receives its
//! own readable records, an archived one (`@@deny`) never, and an
//! anonymous session nothing. The server runs on a second replica of the
//! database, so the events reach it only through the subscription backplane.
//!
//! The client is `cratestack-webtransport`'s native test client, by
//! `#[path]`. Postgres-backed: run it with `CRATESTACK_REQUIRE_DB=1`, or a
//! missing database is a skip that still prints `ok` (CLAUDE.md, "Critical
//! test gotcha").

use std::collections::BTreeMap;
use std::time::Duration;

use cratestack::webtransport::{CBOR_PROTOCOL, WebTransportConfig, WireCodec};
use cratestack::{
    AuthProvider, CratestackContext, CratestackError, RequestContext, Value, include_server_schema,
};

include_server_schema!(
    "tests/fixtures/subscribe_webtransport.cstack",
    db = Postgres
);

mod support;
#[path = "../../cratestack-webtransport/tests/support/mod.rs"]
mod wt;

use support::pg;

/// `authorization: Bearer <id>` is the caller `<id>`; no header is
/// anonymous.
#[derive(Clone)]
struct BearerId;

impl AuthProvider for BearerId {
    type Error = CratestackError;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        let id = request
            .headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .and_then(|value| value.parse::<i64>().ok());
        core::future::ready(Ok(match id {
            Some(id) => CratestackContext::authenticated([("id".to_owned(), Value::Int(id))]),
            None => CratestackContext::anonymous(),
        }))
    }
}

async fn session(
    url: &str,
    cert: &cratestack::webtransport::CertificateDer<'static>,
    id: Option<i64>,
) -> wt::Client {
    let request = web_transport_quinn::proto::ConnectRequest::new(url.parse::<url::Url>().unwrap())
        .with_protocol(CBOR_PROTOCOL);
    let session = wt::client(cert.clone())
        .connect(request)
        .await
        .expect("connect");
    let mut client = wt::Client::new(session, WireCodec::Cbor).await;
    let headers: BTreeMap<_, _> = id
        .map(|id| ("authorization".to_owned(), format!("Bearer {id}")))
        .into_iter()
        .collect();
    let ready = client.init(headers).await;
    assert!(
        ready.ready.is_some() && ready.id == 0,
        "expected Ready{{0}}, got {ready:?}"
    );
    client
}

#[tokio::test]
async fn model_feeds_over_webtransport_pass_the_read_policy() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    // The tests of this binary reset the same tables.
    let _guard = pg::serial_guard().await;
    let pool = &test_pg.pool;
    for statement in [
        "DROP TABLE IF EXISTS wt_notes",
        "CREATE TABLE wt_notes (id BIGINT PRIMARY KEY, owner_id BIGINT NOT NULL, body TEXT NOT NULL, archived BOOLEAN NOT NULL)",
        "DROP TABLE IF EXISTS cratestack_event_outbox",
    ] {
        cratestack::sqlx::query(statement)
            .execute(pool)
            .await
            .expect(statement);
    }
    // Writes go through replica A; the WebTransport server runs on replica B,
    // with its own pool and so its own backplane listener: every event below
    // crosses replicas through Postgres `NOTIFY` (design §4, slice S2).
    let db = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let pool_b = cratestack::sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect(&test_pg.url)
        .await
        .expect("replica B's own pool");
    let replica_b = cratestack_schema::Cratestack::builder(pool_b).build();

    let (chain, key, _hash) = wt::cert();
    let cert = chain[0].clone();
    let config = WebTransportConfig::new("127.0.0.1:0".parse().unwrap(), chain, key);
    let server = replica_b
        .subscriptions()
        .webtransport(config, BearerId)
        .expect("the server binds");
    let url = format!(
        "https://127.0.0.1:{}/rpc/wt",
        server.local_addr().unwrap().port()
    );
    tokio::spawn(server.serve());

    let owner = session(&url, &cert, Some(1)).await;
    let anonymous = session(&url, &cert, None).await;
    let op = "model.WtNote.subscribe";
    let mut owner_feed = owner.subscribe(1, op, serde_json::Value::Null).await;
    let mut anonymous_feed = anonymous.subscribe(1, op, serde_json::Value::Null).await;
    for feed in [&mut owner_feed, &mut anonymous_feed] {
        let ready = feed.next().await.expect("a Ready frame");
        assert!(
            ready.ready.is_some() && ready.id == 1,
            "expected Ready{{1}}, got {ready:?}"
        );
    }

    for (id, owner_id, archived) in [(1, 1, false), (2, 1, true), (3, 2, false)] {
        db.wt_note()
            .bind(CratestackContext::authenticated([(
                "id".to_owned(),
                Value::Int(owner_id),
            )]))
            .create(cratestack_schema::CreateWtNoteInput {
                id,
                ownerId: owner_id,
                body: format!("note {id}"),
                archived,
            })
            .run()
            .await
            .expect("note created");
    }

    let frame = owner_feed.next().await.expect("an item frame");
    let event = frame.next.expect("an item, not an error or end");
    assert_eq!(event["model"], "WtNote");
    assert_eq!(event["data"]["id"], 1);
    // Nothing else reaches either feed: note 2 is archived, note 3 is owner
    // 2's, and anonymous reads nothing. (`Stream::next` panics after 5s, so
    // absence is read off the raw stream.)
    for (who, feed) in [
        ("owner 1", &mut owner_feed),
        ("anonymous", &mut anonymous_feed),
    ] {
        let more = tokio::time::timeout(
            Duration::from_millis(600),
            feed.recv.read_chunk(64 * 1024, true),
        )
        .await;
        assert!(
            more.is_err(),
            "{who} received more than it may read: {more:?}"
        );
    }
}
