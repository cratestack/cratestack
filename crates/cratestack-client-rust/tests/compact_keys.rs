//! Compact wire keys on the Rust client, against a local axum server that
//! records what it was sent and answers compact or plain on demand.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Response, StatusCode, header};
use axum::routing::post;
use bytes::Bytes;
use cratestack_client_rust::{ClientConfig, CratestackClient, RouteRef};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_cbor::compact::cbor_to_compact;
use cratestack_core::CratestackCodec;
use cratestack_core::compact::{
    COMPACT_CBOR, COMPACT_CBOR_SEQ, Direction, KeyNode, KeyStruct, KeyTables, OpKeys,
};
use serde::{Deserialize, Serialize};
use url::Url;

const OP: &str = "model.Widget.create";
const STREAM_OP: &str = "procedure.ticks";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Widget {
    id: i64,
    name: String,
}

#[derive(Debug, Clone, Default)]
struct Seen {
    accept: Option<String>,
    contract: Option<String>,
    content_type: Option<String>,
    body: Vec<u8>,
}

#[derive(Clone)]
struct Server {
    /// Whether this server speaks compact at all.
    compact: bool,
    seen: Arc<Mutex<Vec<Seen>>>,
    tables: Arc<KeyTables>,
}

fn tables() -> KeyTables {
    let ops = [OP, STREAM_OP].map(|op| OpKeys {
        op: op.to_owned(),
        digest: [7; 32],
        request: KeyNode::Struct(0),
        response: KeyNode::Struct(0),
    });
    KeyTables {
        types: vec![KeyStruct {
            fields: vec![
                ("id".to_owned(), KeyNode::Opaque),
                ("name".to_owned(), KeyNode::Opaque),
            ],
        }],
        ops: ops.into(),
    }
}

fn widget(id: i64) -> Widget {
    Widget {
        id,
        name: format!("w{id}"),
    }
}

fn header_of(headers: &HeaderMap, name: header::HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

async fn handle(State(server): State<Server>, headers: HeaderMap, body: Bytes) -> Response<Body> {
    let accept = header_of(&headers, header::ACCEPT);
    server.seen.lock().unwrap().push(Seen {
        accept: accept.clone(),
        contract: headers
            .get("cratestack-contract")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned),
        content_type: header_of(&headers, header::CONTENT_TYPE),
        body: body.to_vec(),
    });
    let named = CborCodec.encode(&widget(7)).unwrap();
    let wants_compact =
        server.compact && accept.as_deref().is_some_and(|a| a.contains(COMPACT_CBOR));
    let (bytes, ct) = if wants_compact {
        let node = KeyNode::Struct(0);
        (
            cbor_to_compact(&server.tables, &node, &named).unwrap(),
            COMPACT_CBOR,
        )
    } else {
        (named, "application/cbor")
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, ct)
        .body(Body::from(bytes))
        .unwrap()
}

async fn handle_stream(
    State(server): State<Server>,
    headers: HeaderMap,
    _body: Bytes,
) -> Response<Body> {
    let accept = header_of(&headers, header::ACCEPT).unwrap_or_default();
    server.seen.lock().unwrap().push(Seen {
        accept: Some(accept.clone()),
        contract: header_of(
            &headers,
            header::HeaderName::from_static("cratestack-contract"),
        ),
        ..Seen::default()
    });
    let compact = server.compact && accept.contains(COMPACT_CBOR_SEQ);
    let mut out = Vec::new();
    for id in 1..=3 {
        let named = CborCodec.encode(&widget(id)).unwrap();
        out.extend(if compact {
            cbor_to_compact(&server.tables, &KeyNode::Struct(0), &named).unwrap()
        } else {
            named
        });
    }
    let ct = if compact {
        COMPACT_CBOR_SEQ
    } else {
        "application/cbor-seq"
    };
    Response::builder()
        .header(header::CONTENT_TYPE, ct)
        .body(Body::from(out))
        .unwrap()
}

async fn spawn(compact: bool) -> (Url, Arc<Mutex<Vec<Seen>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let server = Server {
        compact,
        seen: seen.clone(),
        tables: Arc::new(tables()),
    };
    let app = Router::new()
        .route("/rpc/model.Widget.create", post(handle))
        .route("/rpc/procedure.ticks", post(handle_stream))
        .with_state(server);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, seen)
}

fn client(url: Url, compact: bool) -> CratestackClient {
    let client = CratestackClient::cbor(ClientConfig::new(url));
    if compact {
        client.with_compact_keys(tables())
    } else {
        client
    }
}

async fn create(client: &CratestackClient) -> Widget {
    client
        .at(RouteRef::rpc(OP))
        .post(&format!("/rpc/{OP}"), &widget(1), &[])
        .await
        .unwrap()
}

#[tokio::test]
async fn off_sends_exactly_the_plain_headers_and_body() {
    let (url, seen) = spawn(true).await;
    let client = client(url, false);
    assert_eq!(create(&client).await, widget(7));
    assert_eq!(create(&client).await, widget(7));
    let seen = seen.lock().unwrap();
    for request in seen.iter() {
        assert_eq!(
            request.accept.as_deref(),
            Some("application/cbor, application/json")
        );
        assert_eq!(request.contract, None);
        assert_eq!(request.content_type.as_deref(), Some("application/cbor"));
        assert_eq!(request.body, CborCodec.encode(&widget(1)).unwrap());
    }
}

#[tokio::test]
async fn on_offers_compact_decodes_it_and_then_sends_compact() {
    let (url, seen) = spawn(true).await;
    let client = client(url, true);
    assert_eq!(create(&client).await, widget(7));
    assert_eq!(create(&client).await, widget(7));
    let seen = seen.lock().unwrap();
    let selector = cratestack_core::ContractSelector::of(&[7; 32]).to_header_value();
    for request in seen.iter() {
        assert_eq!(
            request.accept.as_deref(),
            Some(
                "application/vnd.cratestack.compact+cbor, application/vnd.cratestack.compact+json, application/cbor, application/json"
            )
        );
        assert_eq!(request.contract.as_deref(), Some(selector.as_str()));
    }
    // The first body is still named: nothing has proved the server reads compact.
    assert_eq!(seen[0].content_type.as_deref(), Some("application/cbor"));
    assert_eq!(seen[0].body, CborCodec.encode(&widget(1)).unwrap());
    // The second follows the compact response.
    assert_eq!(seen[1].content_type.as_deref(), Some(COMPACT_CBOR));
    let node = KeyNode::Struct(0);
    let expected =
        cbor_to_compact(&tables(), &node, &CborCodec.encode(&widget(1)).unwrap()).unwrap();
    assert_eq!(seen[1].body, expected);
    assert!(seen[1].body.len() < seen[0].body.len());
    let _ = Direction::Request;
}

#[tokio::test]
async fn a_plain_response_keeps_requests_named() {
    let (url, seen) = spawn(false).await;
    let client = client(url, true);
    assert_eq!(create(&client).await, widget(7));
    assert_eq!(create(&client).await, widget(7));
    let seen = seen.lock().unwrap();
    assert!(
        seen.iter()
            .all(|r| r.content_type.as_deref() == Some("application/cbor"))
    );
    assert!(
        seen.iter()
            .all(|r| r.body == CborCodec.encode(&widget(1)).unwrap())
    );
}

#[tokio::test]
async fn an_unknown_route_stays_plain_even_with_compact_on() {
    let (url, seen) = spawn(true).await;
    let client = client(url, true);
    let _: Widget = client
        .at(RouteRef::rpc("batch"))
        .post("/rpc/model.Widget.create", &widget(1), &[])
        .await
        .unwrap();
    let seen = seen.lock().unwrap();
    assert_eq!(
        seen[0].accept.as_deref(),
        Some("application/cbor, application/json")
    );
    assert_eq!(seen[0].contract, None);
}

#[tokio::test]
async fn an_idempotent_retry_reuses_its_first_encoding() {
    let (url, seen) = spawn(true).await;
    let client = client(url, true);
    let call = |client: &CratestackClient| {
        let client = client.at(RouteRef::rpc(OP));
        async move {
            let _: Widget = client
                .post(
                    &format!("/rpc/{OP}"),
                    &widget(1),
                    &[("Idempotency-Key", "k-1")],
                )
                .await
                .unwrap();
        }
    };
    call(&client).await;
    // The response flipped the op to compact; the retry must still be named.
    call(&client).await;
    let seen = seen.lock().unwrap();
    assert_eq!(seen[1].content_type.as_deref(), Some("application/cbor"));
    assert_eq!(seen[0].body, seen[1].body);
}

#[tokio::test]
async fn stream_items_are_decoded_from_a_compact_sequence() {
    let (url, seen) = spawn(true).await;
    let client = client(url, true);
    let mut rx = client
        .at(RouteRef::rpc(STREAM_OP))
        .post_list_streamed::<_, Widget>(&format!("/rpc/{STREAM_OP}"), &widget(0), &[])
        .await
        .unwrap();
    let mut got = Vec::new();
    while let Some(item) = rx.recv().await {
        got.push(item.unwrap());
    }
    assert_eq!(got, vec![widget(1), widget(2), widget(3)]);
    let seen = seen.lock().unwrap();
    assert!(
        seen[0]
            .accept
            .as_deref()
            .unwrap()
            .starts_with(COMPACT_CBOR_SEQ)
    );
    assert!(seen[0].contract.is_some());
}

#[tokio::test]
async fn stream_items_stay_named_against_a_plain_server() {
    let (url, _) = spawn(false).await;
    let client = client(url, true);
    let mut rx = client
        .at(RouteRef::rpc(STREAM_OP))
        .post_list_streamed::<_, Widget>(&format!("/rpc/{STREAM_OP}"), &widget(0), &[])
        .await
        .unwrap();
    let mut got = Vec::new();
    while let Some(item) = rx.recv().await {
        got.push(item.unwrap());
    }
    assert_eq!(got.len(), 3);
}
