//! The `application/cbor-seq` subscription reader end to end (cratestack#1001)
//! against a real axum server: the request asks for the media type, items
//! keep their CBOR shapes, `end` closes cleanly, and a body that just stops is
//! an error, not a quiet end.

use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Response, header};
use axum::routing::get;
use bytes::Bytes;
use cratestack_client_rust::{
    ClientConfig, ClientError, CratestackClient, SubscriptionTarget, SubscriptionTransport,
};
use cratestack_codec_cbor::CborCodec;
use cratestack_core::rpc::RpcFrameValue;
use cratestack_core::subscription_wire::ServerFrame;
use cratestack_core::{CratestackCodec, CratestackError};
use serde::Deserialize;
use url::Url;

const ID: u128 = 0x0123_4567_89ab_cdef_0123_4567_89ab_cdef;

#[derive(Debug, Deserialize, PartialEq)]
struct Reading {
    id: uuid::Uuid,
    n: i64,
}

fn encode(frame: &ServerFrame) -> Vec<u8> {
    CborCodec.encode(frame).unwrap()
}

fn reading() -> RpcFrameValue {
    RpcFrameValue::Object(
        [
            (
                "id".to_owned(),
                RpcFrameValue::Bytes(ID.to_be_bytes().to_vec()),
            ),
            ("n".to_owned(), RpcFrameValue::Number(7.into())),
        ]
        .into(),
    )
}

fn respond(chunks: Vec<Vec<u8>>) -> Response<Body> {
    let stream = futures_util::stream::iter(
        chunks
            .into_iter()
            .map(|chunk| Ok::<_, Infallible>(Bytes::from(chunk))),
    );
    Response::builder()
        .header(header::CONTENT_TYPE, "application/cbor-seq")
        .body(Body::from_stream(stream))
        .unwrap()
}

/// What the server saw in `Accept`, echoed in the item of the last frame.
async fn served(headers: HeaderMap) -> Response<Body> {
    assert_eq!(headers[header::ACCEPT], "application/cbor-seq");
    let mut wire = vec![0xf6];
    wire.extend(encode(&ServerFrame::ready(1)));
    wire.extend(encode(&ServerFrame::next(2, reading())));
    wire.push(0xf6);
    wire.extend(encode(&ServerFrame::next(3, reading())));
    wire.extend(encode(&ServerFrame::end(4)));
    // Split mid-frame, so the reader must stitch the chunks.
    let (a, b) = wire.split_at(wire.len() / 2);
    respond(vec![a.to_vec(), b.to_vec()])
}

async fn errored() -> Response<Body> {
    let mut wire = encode(&ServerFrame::ready(1));
    wire.extend(encode(&ServerFrame::err(
        2,
        &CratestackError::Unavailable("subscription lagged".to_owned()),
    )));
    respond(vec![wire])
}

async fn stops() -> Response<Body> {
    let mut wire = encode(&ServerFrame::ready(1));
    wire.extend(encode(&ServerFrame::next(2, reading())));
    respond(vec![wire])
}

async fn truncated() -> Response<Body> {
    let mut wire = encode(&ServerFrame::ready(1));
    let item = encode(&ServerFrame::next(2, reading()));
    wire.extend(&item[..item.len() - 3]);
    respond(vec![wire])
}

struct DropFlag(Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

async fn endless(State(dropped): State<Arc<AtomicBool>>) -> Response<Body> {
    let guard = DropFlag(dropped);
    let stream = async_stream::stream! {
        let _guard = guard;
        yield Ok::<_, Infallible>(Bytes::from(encode(&ServerFrame::ready(1))));
        // A byte string that declares 4 GiB, then never finishes.
        yield Ok(Bytes::from_static(&[0x5a, 0xff, 0xff, 0xff, 0xff]));
        loop {
            yield Ok(Bytes::from(vec![7u8; 1024]));
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    };
    Response::builder()
        .header(header::CONTENT_TYPE, "application/cbor-seq")
        .body(Body::from_stream(stream))
        .unwrap()
}

async fn client(app: Router, cap: usize) -> CratestackClient<CborCodec> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await });
    let config = ClientConfig::new(Url::parse(&format!("http://{addr}")).unwrap())
        .with_subscription_transport(SubscriptionTransport::CborSeq)
        .with_max_subscription_frame_bytes(cap);
    CratestackClient::new(config, CborCodec)
}

fn target(name: &str) -> SubscriptionTarget {
    SubscriptionTarget {
        op_id: format!("procedure.{name}"),
        input: serde_json::json!({}),
        sse_path: format!("/rpc/subscribe/procedure.{name}"),
        sse_query: None,
        socket_path: "/rpc/ws",
    }
}

fn app() -> Router {
    Router::new()
        .route("/rpc/subscribe/procedure.served", get(served))
        .route("/rpc/subscribe/procedure.errored", get(errored))
        .route("/rpc/subscribe/procedure.stops", get(stops))
        .route("/rpc/subscribe/procedure.truncated", get(truncated))
}

#[tokio::test]
async fn items_keep_their_cbor_shapes_and_end_closes_the_stream() {
    let client = client(app(), 1 << 20).await;
    let mut subscription = client.subscribe::<Reading>(target("served")).await.unwrap();
    let want = Reading {
        id: uuid::Uuid::from_u128(ID),
        n: 7,
    };
    assert_eq!(subscription.recv().await.unwrap().unwrap(), want);
    assert_eq!(subscription.recv().await.unwrap().unwrap(), want);
    assert!(subscription.recv().await.is_none(), "`end` is a clean end");
}

#[tokio::test]
async fn an_err_frame_is_the_last_item() {
    let client = client(app(), 1 << 20).await;
    let mut subscription = client
        .subscribe::<Reading>(target("errored"))
        .await
        .unwrap();
    let error = subscription.recv().await.unwrap().unwrap_err();
    assert!(
        matches!(error, ClientError::Remote { status, .. } if status == 503),
        "{error:?}"
    );
    assert!(subscription.recv().await.is_none());
}

#[tokio::test]
async fn a_body_that_stops_without_a_terminal_frame_is_an_error() {
    let client = client(app(), 1 << 20).await;
    let mut subscription = client.subscribe::<Reading>(target("stops")).await.unwrap();
    assert!(subscription.recv().await.unwrap().is_ok());
    let error = subscription.recv().await.unwrap().unwrap_err();
    assert!(
        matches!(error, ClientError::InvalidResponse(_)),
        "{error:?}"
    );
    assert!(subscription.recv().await.is_none());
}

#[tokio::test]
async fn a_truncated_frame_at_the_end_is_an_error() {
    let client = client(app(), 1 << 20).await;
    let mut subscription = client
        .subscribe::<Reading>(target("truncated"))
        .await
        .unwrap();
    let error = subscription.recv().await.unwrap().unwrap_err();
    assert!(
        matches!(&error, ClientError::InvalidResponse(m) if m.contains("middle of a frame")),
        "{error:?}"
    );
}

#[tokio::test]
async fn an_endless_frame_ends_with_event_too_large_and_drops_the_connection() {
    let dropped = Arc::new(AtomicBool::new(false));
    let app = Router::new()
        .route("/rpc/subscribe/procedure.endless", get(endless))
        .with_state(dropped.clone());
    let client = client(app, 16 * 1024).await;
    let mut subscription = client
        .subscribe::<Reading>(target("endless"))
        .await
        .unwrap();
    let refusal = tokio::time::timeout(Duration::from_secs(10), subscription.recv())
        .await
        .expect("the client must stop reading, not buffer forever")
        .expect("an error, not a clean end");
    assert!(
        matches!(refusal, Err(ClientError::EventTooLarge { limit: 16384 })),
        "{refusal:?}"
    );
    assert!(subscription.recv().await.is_none());
    for _ in 0..100 {
        if dropped.load(Ordering::SeqCst) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the server's response body was never dropped: the connection stayed open");
}
