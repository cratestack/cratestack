//! A subscription over a WebSocket (`docs/design/subscriptions-websocket.md`
//! §6): one socket per subscription, CBOR frames. The upgrade carries the
//! request authorizer's headers for `GET <socket path>`, and an empty
//! `Init` tells the server to authenticate them; the call resolves at the
//! subscription's `Ready`. Dropping the subscription sends `Cancel`.

use cratestack_codec_cbor::CborCodec;
use cratestack_core::CratestackCodec;
use cratestack_core::subscription_wire::{ClientFrame, ServerFrame};
use futures_util::{SinkExt, StreamExt};
use reqwest::Method;
use serde::de::DeserializeOwned;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use super::frames::{item, rpc_error_to_client};
use super::{ClientSubscription, SubscriptionTarget};
use crate::client::CratestackClient;
use crate::codec::HttpClientCodec;
use crate::error::ClientError;

const PROTOCOL: &str = "cratestack.rpc.v1+cbor";
const ID: u64 = 1;

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Why a socket subscription did not open.
pub(super) enum Failure {
    /// No socket: the route is missing, an upgrade is blocked, the network
    /// failed. `Auto` falls back to SSE.
    Unreachable(ClientError),
    /// The server answered: a refusal, a bad input. Never a fallback.
    Refused(ClientError),
}

impl Failure {
    pub(super) fn into_error(self) -> ClientError {
        match self {
            Self::Unreachable(error) | Self::Refused(error) => error,
        }
    }
}

pub(super) async fn open<C: HttpClientCodec, T: DeserializeOwned + Send + 'static>(
    client: &CratestackClient<C>,
    target: &SubscriptionTarget,
) -> Result<ClientSubscription<T>, Failure> {
    crate::client::ensure_crypto_provider();
    let mut url = client.config.base_url.clone();
    let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
    url.set_scheme(scheme).map_err(|()| {
        Failure::Unreachable(ClientError::BadInput(
            "base URL cannot be a socket".to_owned(),
        ))
    })?;
    url.set_path(target.socket_path);
    url.set_query(None);
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|error| Failure::Unreachable(ClientError::BadInput(error.to_string())))?;
    let headers = client
        .build_header_map(&Method::GET, target.socket_path, None, None, &[], "*/*")
        .await
        .map_err(Failure::Refused)?;
    for (name, value) in &headers {
        if name != reqwest::header::ACCEPT {
            request.headers_mut().insert(name.clone(), value.clone());
        }
    }
    request.headers_mut().insert(
        "sec-websocket-protocol",
        PROTOCOL.parse().expect("a valid header"),
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|error| {
            Failure::Unreachable(ClientError::InvalidResponse(format!("WebSocket: {error}")))
        })?;

    send(&mut socket, &ClientFrame::init(Default::default()))
        .await
        .map_err(Failure::Unreachable)?;
    expect_ready(&mut socket, 0).await?;
    let request = ClientFrame::request(ID, target.op_id.clone(), target.input.clone());
    send(&mut socket, &request)
        .await
        .map_err(Failure::Unreachable)?;
    expect_ready(&mut socket, ID).await?;

    let (tx, rx) = tokio::sync::mpsc::channel(16);
    tokio::spawn(pump(socket, tx));
    Ok(rx)
}

async fn expect_ready(socket: &mut Socket, id: u64) -> Result<(), Failure> {
    match recv(socket).await {
        Some(Ok(frame)) if frame.id == id && frame.ready.is_some() => Ok(()),
        Some(Ok(frame)) if frame.err.is_some() => Err(Failure::Refused(rpc_error_to_client(
            frame.err.expect("checked"),
        ))),
        Some(Ok(_)) => Err(Failure::Refused(ClientError::InvalidResponse(format!(
            "expected Ready{{{id}}}"
        )))),
        Some(Err(error)) => Err(Failure::Unreachable(error)),
        None => Err(Failure::Unreachable(ClientError::InvalidResponse(
            "the socket closed before Ready".to_owned(),
        ))),
    }
}

async fn pump<T: DeserializeOwned + Send + 'static>(
    mut socket: Socket,
    tx: tokio::sync::mpsc::Sender<Result<T, ClientError>>,
) {
    loop {
        let frame = tokio::select! {
            frame = recv(&mut socket) => frame,
            () = tx.closed() => {
                let _ = send(&mut socket, &ClientFrame::cancel(ID)).await;
                let _ = socket.close(None).await;
                return;
            }
        };
        let (item, last) = match frame {
            Some(Ok(frame)) if frame.id != ID => continue,
            Some(Ok(frame)) if frame.next.is_some() => (item(frame.next.expect("checked")), false),
            Some(Ok(frame)) if frame.err.is_some() => {
                (Err(rpc_error_to_client(frame.err.expect("checked"))), true)
            }
            Some(Ok(_)) | None => {
                let _ = socket.close(None).await;
                return;
            }
            Some(Err(error)) => (Err(error), true),
        };
        let last = last || item.is_err();
        if tx.send(item).await.is_err() || last {
            let _ = socket.close(None).await;
            return;
        }
    }
}

async fn send(socket: &mut Socket, frame: &ClientFrame) -> Result<(), ClientError> {
    let bytes = CborCodec.encode(frame)?;
    socket
        .send(Message::Binary(bytes.into()))
        .await
        .map_err(|error| ClientError::InvalidResponse(format!("WebSocket: {error}")))
}

/// The next server frame; `None` once the socket closes.
async fn recv(socket: &mut Socket) -> Option<Result<ServerFrame, ClientError>> {
    loop {
        return match socket.next().await? {
            Ok(Message::Binary(bytes)) => Some(CborCodec.decode(&bytes).map_err(ClientError::from)),
            Ok(Message::Close(_)) => None,
            Ok(_) => continue,
            Err(error) => Some(Err(ClientError::InvalidResponse(format!(
                "WebSocket: {error}"
            )))),
        };
    }
}
