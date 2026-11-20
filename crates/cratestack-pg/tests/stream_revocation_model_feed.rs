//! A `@@subscribe` model feed ends when its principal is revoked, over the
//! generated server and the generated Rust client (cratestack#1381): a revoke
//! ends the live feed with its typed reason, and a tombstone refuses the
//! reopen. Over SSE the feed's own bound is the only thing watching it (no
//! socket session shares the key), so this is the test that fails when the
//! generated model feed opens unwatched. Over WebSocket the client reads the
//! same typed end.
//!
//! Postgres-backed: run it with `CRATESTACK_REQUIRE_DB=1`, or a missing
//! database is a skip that still prints `ok` (CLAUDE.md, "Critical test
//! gotcha"). A skip reports `0.00s`.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use cratestack::client_rust::{
    AuthorizationRequest, ClientConfig, ClientError, CratestackClient, RequestAuthorizer,
    SubscriptionTransport,
};
use cratestack::websocket::WebSocketConfig;
use cratestack::{
    AuthProvider, CratestackContext, CratestackError, RequestContext, RevocationKey,
    StreamEndReason, SubscriptionStream, Value, include_server_schema,
};
use cratestack_codec_cbor::CborCodec;

include_server_schema!("tests/fixtures/subscribe_websocket.cstack", db = Postgres);

mod support;

use cratestack_schema::procedures::lines;
use support::pg;

/// `authorization: Bearer <id>` is caller `<id>`, user `u<id>`, session `s<id>`.
#[derive(Clone)]
struct Caller;

impl AuthProvider for Caller {
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
        core::future::ready(match id {
            Some(id) => Ok(CratestackContext::authenticated([
                ("id".to_owned(), Value::Int(id)),
                ("sub".to_owned(), Value::String(format!("u{id}"))),
                ("sid".to_owned(), Value::String(format!("s{id}"))),
            ])),
            None => Err(CratestackError::Unauthorized("no credentials".to_owned())),
        })
    }
}

#[derive(Clone)]
struct Lines;

impl cratestack_schema::procedures::ProcedureRegistry for Lines {
    async fn lines(
        &self,
        db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: lines::Args,
        _authorized: lines::Authorized,
    ) -> Result<SubscriptionStream<lines::Output>, CratestackError> {
        db.topics().room_line(&args.args.roomId).subscribe().await
    }
}

struct AsCallerOne;

#[async_trait::async_trait]
impl RequestAuthorizer for AsCallerOne {
    async fn authorize(
        &self,
        _request: &AuthorizationRequest,
    ) -> Result<Vec<(String, String)>, ClientError> {
        Ok(vec![("authorization".to_owned(), "Bearer 1".to_owned())])
    }
}

struct Served {
    revocations: cratestack::StreamRevocations,
    base: String,
}

impl Served {
    fn client(&self, transport: SubscriptionTransport) -> cratestack_schema::client::Client {
        let config =
            ClientConfig::new(self.base.parse().unwrap()).with_subscription_transport(transport);
        cratestack_schema::client::Client::new(
            CratestackClient::cbor(config).with_request_authorizer(Arc::new(AsCallerOne)),
        )
    }
}

async fn serve(pool: &cratestack::sqlx::PgPool) -> Served {
    for statement in [
        "DROP TABLE IF EXISTS ws_notes",
        "CREATE TABLE ws_notes (id BIGINT PRIMARY KEY, owner_id BIGINT NOT NULL, body TEXT NOT NULL)",
        "DROP TABLE IF EXISTS cratestack_event_outbox",
    ] {
        cratestack::sqlx::query(statement)
            .execute(pool)
            .await
            .expect(statement);
    }
    let db = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let revocations = db.stream_revocations().clone();
    let app = cratestack_schema::axum::rpc_router(
        db.clone(),
        Lines,
        (),
        CborCodec,
        Caller,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
    .merge(
        db.subscriptions(Lines)
            .websocket(WebSocketConfig::new(), Caller),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    tokio::spawn(async move { cratestack::axum::serve(listener, app).await.unwrap() });
    Served { revocations, base }
}

const WITHIN: Duration = Duration::from_secs(5);

#[tokio::test]
async fn a_revoke_ends_a_live_model_feed_and_a_tombstone_refuses_the_reopen() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let _guard = pg::serial_guard().await;
    let served = serve(&test_pg.pool).await;

    let mut notes = served
        .client(SubscriptionTransport::Sse)
        .ws_notes()
        .subscribe(&Default::default())
        .await
        .expect("the feed opens");
    let ended = served.revocations.revoke(
        RevocationKey::Session("s1".to_owned()),
        StreamEndReason::DeviceRevoked,
        Some(SystemTime::now() + Duration::from_secs(60)),
    );
    assert_eq!(ended, 1, "the live model feed is the one stream watched");

    let error = tokio::time::timeout(WITHIN, notes.recv())
        .await
        .expect("the feed ends in time")
        .expect("a terminal error, not a quiet end")
        .expect_err("the revoke is the feed's end");
    assert_eq!(
        error.stream_end_reason(),
        Some(StreamEndReason::DeviceRevoked)
    );

    // The tombstone refuses a reopen with the cached credential.
    let Err(refused) = served
        .client(SubscriptionTransport::Sse)
        .ws_notes()
        .subscribe(&Default::default())
        .await
    else {
        panic!("a revoked principal must not reopen the feed");
    };
    let cratestack::client_rust::RpcClientError::Remote(refused) = refused else {
        panic!("the refusal is the server's answer: {refused:?}");
    };
    assert_eq!(
        refused.body.details.expect("details")["reason"],
        "device_revoked"
    );
}

#[tokio::test]
async fn the_rust_client_reads_the_typed_end_of_a_revoked_websocket_feed() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let _guard = pg::serial_guard().await;
    let served = serve(&test_pg.pool).await;

    for _ in 0..20 {
        let mut notes = served
            .client(SubscriptionTransport::WebSocket)
            .ws_notes()
            .subscribe(&Default::default())
            .await
            .expect("the feed opens over the socket");
        // The socket's own session and the feed under it are both watched.
        assert_eq!(
            served.revocations.revoke(
                RevocationKey::Session("s1".to_owned()),
                StreamEndReason::SessionEnded,
                None,
            ),
            2
        );
        let error = tokio::time::timeout(WITHIN, notes.recv())
            .await
            .expect("the feed ends in time")
            .expect("a terminal error, never a quiet end")
            .expect_err("the revoke is the feed's end");
        assert_eq!(
            error.stream_end_reason(),
            Some(StreamEndReason::SessionEnded),
            "{error:?}"
        );
    }
}
