//! A schema declaring `extension websocket { }` serves its subscriptions
//! over WebSocket through the generated dispatcher
//! (`db.subscriptions(registry).websocket(config, auth)`, slice S5): a model
//! feed and a subscription procedure multiplexed on one socket, each with
//! its own policy, and a refused `Init` closing with 4401.
//!
//! Postgres-backed: run it with `CRATESTACK_REQUIRE_DB=1`, or a missing
//! database is a skip that still prints `ok` (CLAUDE.md, "Critical test
//! gotcha").

use std::collections::BTreeMap;
use std::time::Duration;

use cratestack::websocket::{CBOR_PROTOCOL, ClientFrame, ServerFrame, WebSocketConfig};
use cratestack::{
    AuthProvider, CratestackCodec, CratestackContext, CratestackError, RequestContext,
    SubscriptionStream, Value, include_server_schema,
};
use cratestack_codec_cbor::CborCodec;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

include_server_schema!("tests/fixtures/subscribe_websocket.cstack", db = Postgres);

mod support;

use cratestack_schema::procedures::lines;
use support::pg;

/// `authorization: Bearer <id>` is the caller `<id>`; anything else is
/// refused.
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
        core::future::ready(match id {
            Some(id) => Ok(CratestackContext::authenticated([(
                "id".to_owned(),
                Value::Int(id),
            )])),
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

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect(url: &str) -> Socket {
    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("sec-websocket-protocol", CBOR_PROTOCOL.parse().unwrap());
    tokio_tungstenite::connect_async(request)
        .await
        .expect("upgrade")
        .0
}

async fn send(socket: &mut Socket, frame: &ClientFrame) {
    let bytes = CborCodec.encode(frame).unwrap();
    socket.send(Message::Binary(bytes.into())).await.unwrap();
}

async fn recv(socket: &mut Socket) -> ServerFrame {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("a frame in time")
            .expect("open")
            .expect("a message");
        match message {
            Message::Binary(bytes) => return CborCodec.decode(&bytes).unwrap(),
            Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("expected a frame, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn a_socket_carries_a_feed_and_a_procedure() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let _guard = pg::serial_guard().await;
    for statement in [
        "DROP TABLE IF EXISTS ws_notes",
        "CREATE TABLE ws_notes (id BIGINT PRIMARY KEY, owner_id BIGINT NOT NULL, body TEXT NOT NULL)",
        "DROP TABLE IF EXISTS cratestack_event_outbox",
    ] {
        cratestack::sqlx::query(statement)
            .execute(&test_pg.pool)
            .await
            .expect(statement);
    }
    let db = cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build();
    let app = db
        .subscriptions(Lines)
        .websocket(WebSocketConfig::new(), BearerId);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/rpc/ws", listener.local_addr().unwrap());
    tokio::spawn(async move { cratestack::axum::serve(listener, app).await.unwrap() });

    // No credentials: `{id: 0, err}`, then 4401.
    let mut refused = connect(&url).await;
    send(&mut refused, &ClientFrame::init(BTreeMap::new())).await;
    assert_eq!(
        recv(&mut refused).await.err.unwrap().code,
        "unauthenticated"
    );

    let mut socket = connect(&url).await;
    let credentials = BTreeMap::from([("authorization".to_owned(), "Bearer 1".to_owned())]);
    send(&mut socket, &ClientFrame::init(credentials)).await;
    assert!(recv(&mut socket).await.ready.is_some());
    send(
        &mut socket,
        &ClientFrame::request(
            1,
            "model.WsNote.subscribe",
            cratestack::serde_json::Value::Null,
        ),
    )
    .await;
    send(
        &mut socket,
        &ClientFrame::request(
            2,
            "procedure.lines",
            cratestack::serde_json::json!({"args": {"roomId": "r1"}}),
        ),
    )
    .await;
    let mut ready = [recv(&mut socket).await, recv(&mut socket).await];
    ready.sort_by_key(|frame| frame.id);
    assert!(ready.iter().all(|frame| frame.ready.is_some()), "{ready:?}");

    for (id, owner) in [(1, 2), (2, 1)] {
        db.ws_note()
            .bind(CratestackContext::authenticated([(
                "id".to_owned(),
                Value::Int(owner),
            )]))
            .create(cratestack_schema::CreateWsNoteInput {
                id,
                ownerId: owner,
                body: format!("note {id}"),
            })
            .run()
            .await
            .expect("note created");
    }
    db.topics()
        .room_line("r1")
        .publish(&cratestack_schema::Line {
            body: "hello".to_owned(),
        })
        .await
        .expect("published");

    let mut frames = [recv(&mut socket).await, recv(&mut socket).await];
    frames.sort_by_key(|frame| frame.id);
    // Caller 1 reads note 2 only (its own); the room line arrives on id 2.
    assert_eq!(frames[0].id, 1);
    assert_eq!(frames[0].next.as_ref().unwrap()["data"]["id"], 2);
    assert_eq!(frames[1].id, 2);
    assert_eq!(frames[1].next.as_ref().unwrap()["body"], "hello");
}
