//! Fixtures for `tests/rate_limit.rs`: a scripted limiter, the stub auth and
//! dispatcher, and a raw CBOR WebSocket client.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use cratestack_core::subscription_wire::{
    SubscriptionCharge, SubscriptionRateLimit, SubscriptionRefusal,
};
use cratestack_core::{
    AuthProvider, CratestackCodec, CratestackContext, CratestackError, RequestContext,
    SubscriptionDispatcher, SubscriptionOpen, SubscriptionStream, Value,
};
use cratestack_websocket::{CBOR_PROTOCOL, ClientFrame, ServerFrame, WebSocketConfig};
use futures_util::{SinkExt, StreamExt, stream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

pub type Seen = Vec<(String, Option<String>, Option<IpAddr>)>;

/// Admits while `allow > 0`, then throttles for 5s; records every charge.
/// `refuse_with` makes it answer an error instead.
#[derive(Debug, Clone, Default)]
pub struct Budget {
    pub allow: Arc<AtomicUsize>,
    pub seen: Arc<Mutex<Seen>>,
    pub fail: Arc<Mutex<bool>>,
}

impl Budget {
    pub fn allowing(n: usize) -> Self {
        let budget = Self::default();
        budget.allow.store(n, Ordering::SeqCst);
        budget
    }
}

#[async_trait]
impl SubscriptionRateLimit for Budget {
    async fn admit(&self, charge: SubscriptionCharge<'_>) -> Result<(), SubscriptionRefusal> {
        self.seen.lock().unwrap().push((
            charge.op_id.to_owned(),
            charge.principal.map(str::to_owned),
            charge.remote,
        ));
        if *self.fail.lock().unwrap() {
            return Err(SubscriptionRefusal::Error(
                CratestackError::PreconditionFailed("no peer".to_owned()),
            ));
        }
        let left = self.allow.load(Ordering::SeqCst);
        if left == 0 {
            return Err(SubscriptionRefusal::Throttled {
                retry_after_secs: 5,
            });
        }
        self.allow.store(left - 1, Ordering::SeqCst);
        Ok(())
    }
}

#[derive(Clone)]
struct Auth;

impl AuthProvider for Auth {
    type Error = CratestackError;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        let ok = request
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            == Some("ok");
        core::future::ready(if ok {
            Ok(CratestackContext::authenticated([(
                "id".to_owned(),
                Value::Int(1),
            )]))
        } else {
            Err(CratestackError::Unauthorized("bad credentials".to_owned()))
        })
    }
}

#[derive(Clone)]
struct Dispatch;

impl SubscriptionDispatcher for Dispatch {
    fn open(
        &self,
        request: SubscriptionOpen,
    ) -> impl core::future::Future<
        Output = Result<SubscriptionStream<serde_json::Value>, CratestackError>,
    > + Send {
        let to = request.input["to"].as_u64().unwrap_or(0);
        core::future::ready(Ok(match request.op_id.as_str() {
            "forever" => Box::pin(stream::pending()) as SubscriptionStream<serde_json::Value>,
            _ => Box::pin(stream::iter((1..=to).map(|n| Ok(serde_json::json!(n)))))
                as SubscriptionStream<serde_json::Value>,
        }))
    }
}

pub type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Serves the binding; `connect_info` decides whether the peer is known.
pub async fn serve(config: WebSocketConfig, connect_info: bool) -> String {
    let app = cratestack_websocket::router(config, Auth, Dispatch);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        if connect_info {
            let app = app.into_make_service_with_connect_info::<SocketAddr>();
            axum::serve(listener, app).await.unwrap()
        } else {
            axum::serve(listener, app).await.unwrap()
        }
    });
    format!("ws://{addr}/rpc/ws")
}

fn encode(frame: &ClientFrame) -> Message {
    let bytes = cratestack_codec_cbor::CborCodec.encode(frame).unwrap();
    Message::Binary(bytes.into())
}

/// Connects and completes `Init`/`Ready`.
pub async fn open_socket(url: &str) -> Socket {
    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("sec-websocket-protocol", CBOR_PROTOCOL.parse().unwrap());
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let headers = BTreeMap::from([("authorization".to_owned(), "ok".to_owned())]);
    socket
        .send(encode(&ClientFrame::init(headers)))
        .await
        .unwrap();
    assert!(recv(&mut socket).await.ready.is_some());
    socket
}

pub async fn request(socket: &mut Socket, id: u64, op: &str, to: u64) {
    let frame = ClientFrame::request(id, op, serde_json::json!({ "to": to }));
    socket.send(encode(&frame)).await.unwrap();
}

pub async fn recv(socket: &mut Socket) -> ServerFrame {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("a frame in time")
            .expect("the socket is open")
            .expect("a message");
        match message {
            Message::Binary(bytes) => {
                return cratestack_codec_cbor::CborCodec.decode(&bytes).unwrap();
            }
            Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("expected a frame, got {other:?}"),
        }
    }
}
