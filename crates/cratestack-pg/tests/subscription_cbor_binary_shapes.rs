//! Subscription items keep `Uuid` and `Bytes` in their CBOR forms over the
//! CBOR sockets (cratestack#1360): a `Uuid` is a 16-byte string and a
//! `Value::Bytes` inside a `Json` field a byte string, exactly as in a
//! unary CBOR response, not text and base64 as `serde_json::to_value`
//! makes them. The JSON bindings (WebSocket text frames, SSE) are byte for
//! byte what they always were.
//!
//! The procedure tests need no database (the pool is lazy and never used);
//! the model-feed one is Postgres-backed: run it with
//! `CRATESTACK_REQUIRE_DB=1`, or a missing database is a skip that still
//! prints `ok` (CLAUDE.md, "Critical test gotcha").

use std::collections::BTreeMap;
use std::time::Duration;

use cratestack::websocket::{CBOR_PROTOCOL, JSON_PROTOCOL, WebSocketConfig};
use cratestack::{
    AuthProvider, CratestackCodec, CratestackContext, CratestackError, RequestContext,
    SubscriptionStream, Value, include_server_schema,
};
use cratestack_codec_cbor::CborCodec;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

include_server_schema!(
    "tests/fixtures/subscription_cbor_binary_shapes.cstack",
    db = Postgres
);

mod support;
#[path = "../../cratestack-webtransport/tests/support/mod.rs"]
mod wt;

use cratestack_schema::procedures::blobs;
use support::pg;

const ID: u128 = 0x0123456789abcdef0123456789abcdef;
const DATA: [u8; 4] = [0, 1, 2, 255];
const EXTRA: [u8; 2] = [9, 8];

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

/// Yields one fixed `Blob`, whatever it is asked: the item cannot be an
/// echo of the (JSON-shaped) request.
#[derive(Clone)]
struct Blobs;

impl cratestack_schema::procedures::ProcedureRegistry for Blobs {
    async fn blobs(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        _args: blobs::Args,
        _authorized: blobs::Authorized,
    ) -> Result<SubscriptionStream<blobs::Output>, CratestackError> {
        let blob = cratestack_schema::Blob {
            id: uuid::Uuid::from_u128(ID),
            data: DATA.to_vec(),
            extra: cratestack::Json(Value::Bytes(EXTRA.to_vec())),
        };
        Ok(Box::pin(futures_util::stream::iter([Ok(blob)])))
    }
}

fn lazy_db() -> cratestack_schema::Cratestack {
    let pool = cratestack::sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused@127.0.0.1:1/none")
        .expect("a lazy pool does not connect");
    cratestack_schema::Cratestack::builder(pool).build()
}

/// The fixed item, as a test-local type that decodes with the CBOR codec's
/// own (non-human-readable) `Uuid` and byte rules.
#[derive(Debug, PartialEq, Deserialize)]
struct Blob {
    id: uuid::Uuid,
    data: Vec<u8>,
    extra: Value,
}

fn fixed() -> Blob {
    Blob {
        id: uuid::Uuid::from_u128(ID),
        data: DATA.to_vec(),
        extra: Value::Bytes(EXTRA.to_vec()),
    }
}

#[derive(Debug, Deserialize)]
struct ItemFrame<T> {
    id: u64,
    next: Option<T>,
}

fn args() -> serde_json::Value {
    serde_json::json!({"args": {"n": 1}})
}

/// Asserts `item` (the raw `next` of a frame, as `Value`) holds the
/// `Uuid` as 16 bytes and the `Json` field's bytes as a byte string.
fn assert_binary_forms(item: &Value) {
    let Value::Map(map) = item else {
        panic!("expected a map, got {item:?}");
    };
    assert_eq!(
        map["id"],
        Value::Bytes(ID.to_be_bytes().to_vec()),
        "a Uuid is 16 bytes on CBOR: {item:?}"
    );
    assert_eq!(
        map["extra"],
        Value::Bytes(EXTRA.to_vec()),
        "bytes in a Json field stay a byte string: {item:?}"
    );
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect(url: &str, protocol: &str) -> Socket {
    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("sec-websocket-protocol", protocol.parse().unwrap());
    tokio_tungstenite::connect_async(request)
        .await
        .expect("upgrade")
        .0
}

async fn serve_ws(db: cratestack_schema::Cratestack) -> String {
    let app = db
        .subscriptions(Blobs)
        .websocket(WebSocketConfig::new(), BearerId);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/rpc/ws", listener.local_addr().unwrap());
    tokio::spawn(async move { cratestack::axum::serve(listener, app).await.unwrap() });
    url
}

async fn send_cbor(socket: &mut Socket, frame: serde_json::Value) {
    let bytes = CborCodec.encode(&frame).unwrap();
    socket.send(Message::Binary(bytes.into())).await.unwrap();
}

/// The next binary frame's bytes.
async fn recv_binary(socket: &mut Socket) -> Vec<u8> {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("a frame in time")
            .expect("open")
            .expect("a message");
        match message {
            Message::Binary(bytes) => return bytes.to_vec(),
            Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("expected a binary frame, got {other:?}"),
        }
    }
}

async fn recv_text(socket: &mut Socket) -> String {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("a frame in time")
            .expect("open")
            .expect("a message");
        match message {
            Message::Text(text) => return text.to_string(),
            Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("expected a text frame, got {other:?}"),
        }
    }
}

fn init(token: &str) -> serde_json::Value {
    serde_json::json!({"id": 0, "init": {"headers": {"authorization": format!("Bearer {token}")}}})
}

/// (a) A raw CBOR socket: the item decodes as its declared type, the way a
/// unary CBOR response does.
#[tokio::test]
async fn a_websocket_item_is_binary_on_the_cbor_wire() {
    let url = serve_ws(lazy_db()).await;
    let mut socket = connect(&url, CBOR_PROTOCOL).await;
    send_cbor(&mut socket, init("1")).await;
    let ready: serde_json::Value = CborCodec.decode(&recv_binary(&mut socket).await).unwrap();
    assert_eq!(ready["ready"], serde_json::json!({}));
    send_cbor(
        &mut socket,
        serde_json::json!({"id": 1, "op": "procedure.blobs", "input": args()}),
    )
    .await;
    let ready = recv_binary(&mut socket).await;
    assert!(
        CborCodec.decode::<serde_json::Value>(&ready).unwrap()["ready"].is_object(),
        "Ready first"
    );
    let item = recv_binary(&mut socket).await;
    let typed: ItemFrame<Blob> = CborCodec
        .decode(&item)
        .unwrap_or_else(|error| panic!("the item decodes as its declared type: {error}"));
    assert_eq!(typed.id, 1);
    assert_eq!(typed.next, Some(fixed()));
    let raw: ItemFrame<Value> = CborCodec.decode(&item).unwrap();
    assert_binary_forms(&raw.next.expect("an item"));
}

/// (b) The same item on a WebTransport stream.
#[tokio::test]
async fn a_webtransport_item_is_binary_on_the_cbor_wire() {
    let (chain, key, _hash) = wt::cert();
    let cert = chain[0].clone();
    let config = cratestack::webtransport::WebTransportConfig::new(
        "127.0.0.1:0".parse().unwrap(),
        chain,
        key,
    );
    let server = lazy_db()
        .subscriptions(Blobs)
        .webtransport(config, BearerId)
        .expect("the server binds");
    let url = format!(
        "https://127.0.0.1:{}/rpc/wt",
        server.local_addr().unwrap().port()
    );
    tokio::spawn(server.serve());

    let request = web_transport_quinn::proto::ConnectRequest::new(url.parse::<url::Url>().unwrap())
        .with_protocol(cratestack::webtransport::CBOR_PROTOCOL);
    let session = wt::client(cert).connect(request).await.expect("connect");
    let mut client = wt::Client::new(session, cratestack::webtransport::WireCodec::Cbor).await;
    let headers = BTreeMap::from([("authorization".to_owned(), "Bearer 1".to_owned())]);
    assert!(client.init(headers).await.ready.is_some());
    // One stream per decode: a frame is read once.
    let mut raw = client.subscribe(1, "procedure.blobs", args()).await;
    let ready: serde_json::Value = raw.next_as().await.expect("Ready");
    assert!(ready["ready"].is_object(), "{ready:?}");
    let item: ItemFrame<Value> = raw.next_as().await.expect("an item frame");
    assert_binary_forms(&item.next.expect("an item"));

    let mut feed = client.subscribe(2, "procedure.blobs", args()).await;
    let _ready: serde_json::Value = feed.next_as().await.expect("Ready");
    let item: ItemFrame<Blob> = feed
        .next_as()
        .await
        .expect("an item frame decoded as its declared type");
    assert_eq!(item.next, Some(fixed()));
}

/// Caller 1, on every request the generated client sends (the socket's
/// upgrade and a WebTransport `Init` included).
struct AsCallerOne;

#[async_trait::async_trait]
impl cratestack::client_rust::RequestAuthorizer for AsCallerOne {
    async fn authorize(
        &self,
        _request: &cratestack::client_rust::AuthorizationRequest,
    ) -> Result<Vec<(String, String)>, cratestack::client_rust::ClientError> {
        Ok(vec![("authorization".to_owned(), "Bearer 1".to_owned())])
    }
}

async fn assert_client_item(
    client: cratestack_schema::client::Client<impl cratestack::client_rust::HttpClientCodec>,
) {
    let mut feed = client
        .procedures()
        .blobs(&blobs::Args {
            args: cratestack_schema::Ask { n: 1 },
        })
        .await
        .expect("the procedure opens");
    let blob = tokio::time::timeout(Duration::from_secs(5), feed.recv())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(blob.id, uuid::Uuid::from_u128(ID));
    assert_eq!(blob.data, DATA);
    assert_eq!(
        blob.extra.0,
        Value::Bytes(EXTRA.to_vec()),
        "the bytes in a Json field survive the socket"
    );
}

/// (c) The generated Rust client over both CBOR bindings.
#[tokio::test]
async fn the_generated_client_reads_binary_items_over_both_bindings() {
    use cratestack::client_rust::{
        ClientConfig, CratestackClient, ServerTrust, SubscriptionTransport,
    };
    let ws_url = serve_ws(lazy_db()).await;
    let base = ws_url.replace("ws://", "http://").replace("rpc/ws", "");
    let client = |config: ClientConfig| {
        cratestack_schema::client::Client::new(
            CratestackClient::cbor(config)
                .with_request_authorizer(std::sync::Arc::new(AsCallerOne)),
        )
    };
    assert_client_item(client(
        ClientConfig::new(base.parse().unwrap())
            .with_subscription_transport(SubscriptionTransport::WebSocket),
    ))
    .await;

    let (chain, key, hash) = wt::cert();
    let config = cratestack::webtransport::WebTransportConfig::new(
        "127.0.0.1:0".parse().unwrap(),
        chain,
        key,
    );
    let server = lazy_db()
        .subscriptions(Blobs)
        .webtransport(config, BearerId)
        .expect("the server binds");
    let wt_url = format!(
        "https://127.0.0.1:{}/rpc/wt",
        server.local_addr().unwrap().port()
    );
    tokio::spawn(server.serve());
    let hash: [u8; 32] = hash.try_into().expect("a SHA-256 hash");
    assert_client_item(client(
        ClientConfig::new(base.parse().unwrap())
            .with_subscription_transport(SubscriptionTransport::WebTransport)
            .with_webtransport(
                wt_url.parse().unwrap(),
                ServerTrust::CertificateHashes(vec![hash]),
            ),
    ))
    .await;
}

const JSON_ITEM: &str =
    r#"{"data":[0,1,2,255],"extra":"CQg=","id":"01234567-89ab-cdef-0123-456789abcdef"}"#;

/// (d) JSON guard, WebSocket text frames: unchanged by the fix.
#[tokio::test]
async fn json_websocket_frames_keep_their_text_forms() {
    let url = serve_ws(lazy_db()).await;
    let mut socket = connect(&url, JSON_PROTOCOL).await;
    socket
        .send(Message::Text(init("1").to_string().into()))
        .await
        .unwrap();
    assert_eq!(recv_text(&mut socket).await, r#"{"id":0,"ready":{}}"#);
    let request = serde_json::json!({"id": 1, "op": "procedure.blobs", "input": args()});
    socket
        .send(Message::Text(request.to_string().into()))
        .await
        .unwrap();
    assert_eq!(recv_text(&mut socket).await, r#"{"id":1,"ready":{}}"#);
    assert_eq!(
        recv_text(&mut socket).await,
        format!(r#"{{"id":1,"next":{JSON_ITEM}}}"#)
    );
}

/// (d) JSON guard, SSE: unchanged by the fix.
#[tokio::test]
async fn sse_frames_keep_their_text_forms() {
    use cratestack::axum::body::{Body, to_bytes};
    use cratestack::axum::http::{Request, StatusCode};
    use tower::util::ServiceExt;

    let router = cratestack_schema::axum::rpc_router(
        lazy_db(),
        Blobs,
        (),
        CborCodec,
        BearerId,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    let response = router
        .oneshot(
            Request::get("/rpc/subscribe/procedure.blobs?args=%7B%22n%22%3A1%7D")
                .header("authorization", "Bearer 1")
                .header("accept", "text/event-stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    assert_eq!(
        std::str::from_utf8(&bytes).unwrap(),
        format!(
            "event: ready\ndata: {{\"id\":1,\"ready\":{{}}}}\n\nevent: message\ndata: {{\"id\":2,\"next\":{JSON_ITEM}}}\n\n"
        )
    );
}

/// (e) A model feed: the event's `eventId` and its record's `Uuid` and
/// bytes arrive in their CBOR forms.
#[tokio::test]
async fn a_model_event_is_binary_on_the_cbor_wire() {
    #[derive(Debug, Deserialize)]
    struct Record {
        id: uuid::Uuid,
        data: Vec<u8>,
    }
    #[derive(Debug, Deserialize)]
    struct Event {
        #[serde(rename = "eventId")]
        event_id: uuid::Uuid,
        data: Record,
    }

    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let _guard = pg::serial_guard().await;
    for statement in [
        "DROP TABLE IF EXISTS bin_notes",
        "CREATE TABLE bin_notes (id UUID PRIMARY KEY, data BYTEA NOT NULL)",
        "DROP TABLE IF EXISTS cratestack_event_outbox",
    ] {
        cratestack::sqlx::query(statement)
            .execute(&test_pg.pool)
            .await
            .expect(statement);
    }
    let db = cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build();
    let url = serve_ws(db.clone()).await;
    let mut socket = connect(&url, CBOR_PROTOCOL).await;
    send_cbor(&mut socket, init("1")).await;
    recv_binary(&mut socket).await;
    send_cbor(
        &mut socket,
        serde_json::json!({"id": 1, "op": "model.BinNote.subscribe"}),
    )
    .await;
    recv_binary(&mut socket).await;

    db.bin_note()
        .bind(CratestackContext::authenticated([(
            "id".to_owned(),
            Value::Int(1),
        )]))
        .create(cratestack_schema::CreateBinNoteInput {
            id: uuid::Uuid::from_u128(ID),
            data: DATA.to_vec(),
        })
        .run()
        .await
        .expect("note created");

    let item = recv_binary(&mut socket).await;
    let frame: ItemFrame<Event> = CborCodec
        .decode(&item)
        .unwrap_or_else(|error| panic!("the event decodes as its declared type: {error}"));
    let event = frame.next.expect("an event");
    assert_eq!(event.data.id, uuid::Uuid::from_u128(ID));
    assert_eq!(event.data.data, DATA);
    assert!(!event.event_id.is_nil());
}
