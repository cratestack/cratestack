//! The socket-level fixtures of `tests/revocation.rs` (cratestack#1381): an
//! authenticating provider, a dispatcher whose streams are bounded the way a
//! generated open bounds them, and frame-level client helpers.

#![allow(dead_code)]

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cratestack_axum::rpc::{StreamBounds, until_principal_ends};
use cratestack_core::rpc::RpcFrameValue;
use cratestack_core::{
    AuthProvider, CratestackCodec, CratestackContext, CratestackError, RequestContext,
    StreamRevocations, SubscriptionDispatcher, SubscriptionOpen, SubscriptionStream, Value,
};
use cratestack_websocket::{CBOR_PROTOCOL, ClientFrame, ServerFrame, WebSocketConfig};
use futures_util::{SinkExt, StreamExt, stream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

/// `gated` is the `long` credential, but its authentication parks until
/// [`GATE`] is opened, after announcing itself on [`ENTERED`]: a refresh the
/// test can hold mid-flight while it revokes.
pub static ENTERED: tokio::sync::Notify = tokio::sync::Notify::const_new();
pub static GATE: tokio::sync::Notify = tokio::sync::Notify::const_new();

/// `authorization: short` is user `u1`, session `s1`, valid for one second;
/// `long` is the same session for an hour; `rotated` is the same user on
/// session `s2` for an hour; anything else is refused.
#[derive(Clone)]
pub struct Auth;

impl AuthProvider for Auth {
    type Error = CratestackError;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        let token = request
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok());
        let (sid, exp) = match token {
            Some("short") => ("s1", Some(now() + 1)),
            Some("long" | "gated") => ("s1", Some(now() + 3600)),
            Some("rotated") => ("s2", Some(now() + 3600)),
            _ => ("", None),
        };
        let gated = token == Some("gated");
        async move {
            if gated {
                ENTERED.notify_one();
                GATE.notified().await;
            }
            match exp {
                Some(exp) => Ok(CratestackContext::authenticated([
                    ("sub".to_owned(), Value::String("u1".to_owned())),
                    ("sid".to_owned(), Value::String(sid.to_owned())),
                    ("exp".to_owned(), Value::Int(exp)),
                ])),
                None => Err(CratestackError::Unauthorized("bad credentials".to_owned())),
            }
        }
    }
}

/// `forever` never ends, bounded the way a generated open bounds it.
#[derive(Clone)]
pub struct Dispatch(pub StreamRevocations);

impl SubscriptionDispatcher for Dispatch {
    fn open<C: CratestackCodec>(
        &self,
        request: SubscriptionOpen,
        _codec: C,
    ) -> impl core::future::Future<
        Output = Result<SubscriptionStream<RpcFrameValue>, CratestackError>,
    > + Send {
        let bounds = StreamBounds::open(&request.ctx, &self.0);
        core::future::ready(bounds.map(|bounds| {
            until_principal_ends(
                Box::pin(stream::pending()) as SubscriptionStream<RpcFrameValue>,
                bounds,
            )
        }))
    }

    fn revocations(&self) -> Option<&StreamRevocations> {
        Some(&self.0)
    }
}

pub type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

pub async fn serve(revocations: &StreamRevocations) -> String {
    let app =
        cratestack_websocket::router(WebSocketConfig::new(), Auth, Dispatch(revocations.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("ws://{addr}/rpc/ws")
}

pub async fn connect(url: &str) -> Socket {
    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("sec-websocket-protocol", CBOR_PROTOCOL.parse().unwrap());
    tokio_tungstenite::connect_async(request).await.unwrap().0
}

pub fn encode(frame: &ClientFrame) -> Message {
    Message::Binary(
        cratestack_codec_cbor::CborCodec
            .encode(frame)
            .unwrap()
            .into(),
    )
}

pub fn credentials(token: &str) -> std::collections::BTreeMap<String, String> {
    [("authorization".to_owned(), token.to_owned())].into()
}

pub enum Next {
    Frame(ServerFrame),
    Closed(u16),
}

pub async fn next(socket: &mut Socket) -> Next {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("a message in time");
        match message {
            Some(Ok(Message::Binary(bytes))) => {
                return Next::Frame(cratestack_codec_cbor::CborCodec.decode(&bytes).unwrap());
            }
            Some(Ok(Message::Close(Some(frame)))) => return Next::Closed(frame.code.into()),
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
            other => panic!("expected a frame or a close, got {other:?}"),
        }
    }
}

pub async fn frame(socket: &mut Socket) -> ServerFrame {
    match next(socket).await {
        Next::Frame(frame) => frame,
        Next::Closed(code) => panic!("closed with {code}"),
    }
}

pub fn reason(frame: &ServerFrame) -> String {
    frame.err.as_ref().unwrap().details.as_ref().unwrap()["reason"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// Connects, authenticates with `token` and opens `streams` subscriptions.
pub async fn session(url: &str, token: &str, streams: u64) -> Socket {
    let mut socket = connect(url).await;
    socket
        .send(encode(&ClientFrame::init(credentials(token))))
        .await
        .unwrap();
    assert!(frame(&mut socket).await.ready.is_some());
    for id in 1..=streams {
        socket
            .send(encode(&ClientFrame::request(
                id,
                "forever",
                serde_json::Value::Null,
            )))
            .await
            .unwrap();
        assert!(frame(&mut socket).await.ready.is_some());
    }
    // Let the server read the client's `Pong` to its first `Ping`: a server
    // that closes with a client frame still unread resets the connection
    // instead of delivering the close, and these tests read the close code.
    tokio::time::sleep(Duration::from_millis(150)).await;
    socket
}

/// Reads to the close. Returns the close code, the reason of the session's
/// `{id: 0, err}` frame, and every per-stream err frame as `(id, reason)`,
/// in the order they arrived. A stream's err frame arriving after the
/// session's is a failure: a client stops reading at `{id: 0, err}`.
pub async fn until_closed(socket: &mut Socket) -> (u16, String, Vec<(u64, String)>) {
    let (mut session_reason, mut stream_reasons) = (None, Vec::new());
    loop {
        match next(socket).await {
            Next::Frame(frame) if frame.id == 0 => session_reason = Some(reason(&frame)),
            Next::Frame(frame) => {
                assert!(
                    session_reason.is_none(),
                    "stream {} ended after the session frame",
                    frame.id
                );
                stream_reasons.push((frame.id, reason(&frame)));
            }
            Next::Closed(code) => {
                return (
                    code,
                    session_reason.expect("an {id: 0, err} frame"),
                    stream_reasons,
                );
            }
        }
    }
}
