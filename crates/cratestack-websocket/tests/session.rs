//! The binding end to end over a real socket: `Init`/`Ready`, a
//! subscription's frames, `Cancel`, id reuse, refusal before `Ready`, the
//! `{id: 0, err}` + 4401 refusal, `Origin`, and JSON over text messages.

use std::collections::BTreeMap;
use std::time::Duration;

use cratestack_core::{
    AuthProvider, CratestackContext, CratestackError, RequestContext, SubscriptionDispatcher,
    SubscriptionOpen, SubscriptionStream, Value,
};
use cratestack_websocket::{
    CBOR_PROTOCOL, ClientFrame, JSON_PROTOCOL, ServerFrame, WebSocketConfig, close_code,
};
use futures_util::{SinkExt, StreamExt, stream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

/// `authorization: ok` is caller 1; anything else is refused.
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

/// `count` yields `input.to` numbers then ends; `forever` never ends;
/// anything else is refused.
#[derive(Clone)]
struct Dispatch;

impl SubscriptionDispatcher for Dispatch {
    fn open(
        &self,
        request: SubscriptionOpen,
    ) -> impl core::future::Future<
        Output = Result<SubscriptionStream<serde_json::Value>, CratestackError>,
    > + Send {
        core::future::ready(match request.op_id.as_str() {
            "count" => {
                let to = request.input["to"].as_u64().unwrap_or(0);
                Ok(
                    Box::pin(stream::iter((1..=to).map(|n| Ok(serde_json::json!(n)))))
                        as SubscriptionStream<serde_json::Value>,
                )
            }
            "forever" => Ok(Box::pin(stream::pending()) as SubscriptionStream<serde_json::Value>),
            other => Err(CratestackError::NotFound(format!(
                "unknown subscription op `{other}`"
            ))),
        })
    }
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn serve(config: WebSocketConfig) -> String {
    let app = cratestack_websocket::router(config, Auth, Dispatch);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("ws://{addr}/rpc/ws")
}

async fn connect(url: &str, protocol: &str, origin: Option<&str>) -> Result<Socket, String> {
    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("sec-websocket-protocol", protocol.parse().unwrap());
    if let Some(origin) = origin {
        request
            .headers_mut()
            .insert("origin", origin.parse().unwrap());
    }
    tokio_tungstenite::connect_async(request)
        .await
        .map(|(socket, _)| socket)
        .map_err(|error| error.to_string())
}

fn encode(frame: &ClientFrame) -> Message {
    use cratestack_core::CratestackCodec;
    Message::Binary(
        cratestack_codec_cbor::CborCodec
            .encode(frame)
            .unwrap()
            .into(),
    )
}

async fn recv(socket: &mut Socket) -> ServerFrame {
    use cratestack_core::CratestackCodec;
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
            Message::Text(text) => return serde_json::from_str(text.as_str()).unwrap(),
            Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("expected a frame, got {other:?}"),
        }
    }
}

fn credentials() -> BTreeMap<String, String> {
    BTreeMap::from([("authorization".to_owned(), "ok".to_owned())])
}

#[tokio::test]
async fn a_socket_multiplexes_subscriptions() {
    let url = serve(WebSocketConfig::new()).await;
    let mut socket = connect(&url, CBOR_PROTOCOL, None).await.unwrap();
    socket
        .send(encode(&ClientFrame::init(credentials())))
        .await
        .unwrap();
    let ready = recv(&mut socket).await;
    assert!(ready.ready.is_some() && ready.id == 0, "{ready:?}");

    socket
        .send(encode(&ClientFrame::request(
            1,
            "forever",
            serde_json::Value::Null,
        )))
        .await
        .unwrap();
    assert!(recv(&mut socket).await.ready.is_some());
    socket
        .send(encode(&ClientFrame::request(
            2,
            "count",
            serde_json::json!({"to": 2}),
        )))
        .await
        .unwrap();
    // An id in use is refused; the subscription holding it is untouched.
    socket
        .send(encode(&ClientFrame::request(
            1,
            "count",
            serde_json::Value::Null,
        )))
        .await
        .unwrap();
    socket
        .send(encode(&ClientFrame::request(
            3,
            "missing",
            serde_json::Value::Null,
        )))
        .await
        .unwrap();
    let mut frames = Vec::new();
    for _ in 0..6 {
        frames.push(recv(&mut socket).await);
    }
    let of = |id: u64| {
        frames
            .iter()
            .filter(move |f| f.id == id)
            .collect::<Vec<_>>()
    };
    let two = of(2);
    assert!(two[0].ready.is_some());
    assert_eq!(two[1].next, Some(serde_json::json!(1)));
    assert_eq!(two[2].next, Some(serde_json::json!(2)));
    assert!(two[3].end.is_some(), "a stream that ends sends `end`");
    assert_eq!(of(1)[0].err.as_ref().unwrap().code, "invalid_argument");
    assert_eq!(of(3)[0].err.as_ref().unwrap().code, "not_found");

    socket.send(encode(&ClientFrame::cancel(1))).await.unwrap();
    let ended = recv(&mut socket).await;
    assert!(ended.id == 1 && ended.end.is_some(), "{ended:?}");
}

#[tokio::test]
async fn a_refusal_sends_its_error_then_closes_4401() {
    let url = serve(WebSocketConfig::new()).await;
    let mut socket = connect(&url, CBOR_PROTOCOL, None).await.unwrap();
    socket
        .send(encode(&ClientFrame::init(BTreeMap::new())))
        .await
        .unwrap();
    let refused = recv(&mut socket).await;
    assert_eq!(refused.id, 0);
    assert_eq!(refused.err.as_ref().unwrap().code, "unauthenticated");
    match socket.next().await {
        Some(Ok(Message::Close(Some(frame)))) => {
            assert_eq!(u16::from(frame.code), close_code::UNAUTHENTICATED as u16)
        }
        other => panic!("expected a 4401 close, got {other:?}"),
    }
}

#[tokio::test]
async fn json_rides_text_messages_and_origin_is_checked() {
    let url = serve(WebSocketConfig::new()).await;
    assert!(
        connect(&url, JSON_PROTOCOL, Some("https://evil.example"))
            .await
            .is_err()
    );
    let mut socket = connect(&url, JSON_PROTOCOL, None).await.unwrap();
    let init = serde_json::to_string(&ClientFrame::init(credentials())).unwrap();
    socket.send(Message::Text(init.into())).await.unwrap();
    assert!(recv(&mut socket).await.ready.is_some());
    let request = serde_json::to_string(&ClientFrame::request(
        5,
        "count",
        serde_json::json!({"to": 1}),
    ))
    .unwrap();
    socket.send(Message::Text(request.into())).await.unwrap();
    assert!(recv(&mut socket).await.ready.is_some());
    assert_eq!(recv(&mut socket).await.next, Some(serde_json::json!(1)));
}
