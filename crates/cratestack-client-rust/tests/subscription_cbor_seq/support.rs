//! The fake server and client the cbor-seq reader tests share.

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
    ClientConfig, CratestackClient, SubscriptionTarget, SubscriptionTransport,
};
use cratestack_codec_cbor::CborCodec;
use cratestack_core::rpc::RpcFrameValue;
use cratestack_core::subscription_wire::ServerFrame;
use cratestack_core::{CratestackCodec, CratestackError};
use serde::Deserialize;
use url::Url;

pub const ID: u128 = 0x0123_4567_89ab_cdef_0123_4567_89ab_cdef;

#[derive(Debug, Deserialize, PartialEq)]
pub struct Reading {
    pub id: uuid::Uuid,
    pub n: i64,
}

pub fn encode(frame: &ServerFrame) -> Vec<u8> {
    CborCodec.encode(frame).unwrap()
}

pub fn reading() -> RpcFrameValue {
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

pub fn respond(chunks: Vec<Vec<u8>>) -> Response<Body> {
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
pub async fn served(headers: HeaderMap) -> Response<Body> {
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

pub async fn errored() -> Response<Body> {
    let mut wire = encode(&ServerFrame::ready(1));
    wire.extend(encode(&ServerFrame::err(
        2,
        &CratestackError::Unavailable("subscription lagged".to_owned()),
    )));
    respond(vec![wire])
}

pub async fn stops() -> Response<Body> {
    let mut wire = encode(&ServerFrame::ready(1));
    wire.extend(encode(&ServerFrame::next(2, reading())));
    respond(vec![wire])
}

/// An SSE body answered to a cbor-seq request.
pub async fn wrong_media_type() -> Response<Body> {
    Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from(
            "event: ready\ndata: {\"id\":1,\"ready\":{}}\n\n",
        ))
        .unwrap()
}

/// A feed whose first frame is an item, not `ready`.
pub async fn next_first() -> Response<Body> {
    respond(vec![encode(&ServerFrame::next(1, reading()))])
}

/// A feed that refuses before `ready`: the open itself fails.
pub async fn err_first() -> Response<Body> {
    respond(vec![encode(&ServerFrame::err(
        1,
        &CratestackError::Forbidden("not yours".to_owned()),
    ))])
}

/// The media type with a parameter and odd case, which is still cbor-seq.
pub async fn media_type_params() -> Response<Body> {
    let mut wire = encode(&ServerFrame::ready(1));
    wire.extend(encode(&ServerFrame::end(2)));
    let mut response = respond(vec![wire]);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        "Application/CBOR-Seq; charset=binary".parse().unwrap(),
    );
    response
}

pub async fn truncated() -> Response<Body> {
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

pub async fn endless(State(dropped): State<Arc<AtomicBool>>) -> Response<Body> {
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

pub async fn client(app: Router, cap: usize) -> CratestackClient<CborCodec> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await });
    let config = ClientConfig::new(Url::parse(&format!("http://{addr}")).unwrap())
        .with_subscription_transport(SubscriptionTransport::CborSeq)
        .with_max_subscription_frame_bytes(cap);
    CratestackClient::new(config, CborCodec)
}

pub fn target(name: &str) -> SubscriptionTarget {
    SubscriptionTarget {
        op_id: format!("procedure.{name}"),
        input: serde_json::json!({}),
        sse_path: format!("/rpc/subscribe/procedure.{name}"),
        sse_query: None,
        socket_path: "/rpc/ws",
        frames: None,
        seal_route: String::new(),
    }
}

pub fn app() -> Router {
    Router::new()
        .route("/rpc/subscribe/procedure.served", get(served))
        .route("/rpc/subscribe/procedure.errored", get(errored))
        .route("/rpc/subscribe/procedure.stops", get(stops))
        .route("/rpc/subscribe/procedure.truncated", get(truncated))
        .route(
            "/rpc/subscribe/procedure.wrong_media_type",
            get(wrong_media_type),
        )
        .route("/rpc/subscribe/procedure.next_first", get(next_first))
        .route("/rpc/subscribe/procedure.err_first", get(err_first))
        .route(
            "/rpc/subscribe/procedure.media_type_params",
            get(media_type_params),
        )
}
