//! A WebSocket that closes after `{id: 0, err}` ends the subscription with
//! that error, typed reason and all (cratestack#1381). A server whose own
//! per-stream `err` frame was lost to the close (it races it) leaves the
//! session frame as the only word a client gets: reading past it, then
//! ending cleanly, would hand a revoked stream's consumer a quiet end.
#![cfg(feature = "ws")]
// The handshake callback's `Err` type is tungstenite's, not ours to shrink.
#![allow(clippy::result_large_err)]

use cratestack_client_rust::SubscriptionTransport;
use cratestack_client_rust::{ClientConfig, CratestackClient, SubscriptionTarget};
use cratestack_codec_cbor::CborCodec;
use cratestack_core::subscription_wire::{ClientFrame, ServerFrame};
use cratestack_core::{CratestackCodec, StreamEndNotice, StreamEndReason};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

fn binary(frame: &ServerFrame) -> Message {
    Message::Binary(CborCodec.encode(frame).unwrap().into())
}

/// Serves one socket: `Ready` for the `Init` and the `Request`, then `then`.
async fn serve(then: Vec<Message>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let echo = |request: &Request, mut response: Response| {
            if let Some(protocol) = request.headers().get("sec-websocket-protocol") {
                response
                    .headers_mut()
                    .insert("sec-websocket-protocol", protocol.clone());
            }
            Ok(response)
        };
        let mut socket = tokio_tungstenite::accept_hdr_async(tcp, echo)
            .await
            .unwrap();
        for ready in [0, 1] {
            let Some(Ok(Message::Binary(bytes))) = socket.next().await else {
                return;
            };
            let _: ClientFrame = CborCodec.decode(&bytes).unwrap();
            socket
                .send(binary(&ServerFrame::ready(ready)))
                .await
                .unwrap();
        }
        for message in then {
            socket.send(message).await.unwrap();
        }
        // Hold the socket until the client has read, then let it drop.
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), socket.next()).await;
    });
    format!("http://{addr}")
}

fn target() -> SubscriptionTarget {
    SubscriptionTarget {
        op_id: "procedure.forever".to_owned(),
        input: serde_json::json!({}),
        sse_path: "/rpc/subscribe/procedure.forever".to_owned(),
        sse_query: None,
        socket_path: "/rpc/ws",
        frames: None,
        seal_route: String::new(),
    }
}

async fn subscribe(
    base: String,
) -> tokio::sync::mpsc::Receiver<Result<serde_json::Value, cratestack_client_rust::ClientError>> {
    let config = ClientConfig::new(base.parse().unwrap())
        .with_subscription_transport(SubscriptionTransport::WebSocket);
    CratestackClient::new(config, CborCodec)
        .subscribe(target())
        .await
        .expect("the subscription opens")
}

#[tokio::test]
async fn a_session_frame_then_a_close_surfaces_the_typed_end() {
    let end = StreamEndNotice::new(StreamEndReason::SessionEnded).into_error();
    let base = serve(vec![
        binary(&ServerFrame::err(0, &end)),
        Message::Close(Some(CloseFrame {
            code: CloseCode::from(4401),
            reason: "session_ended".into(),
        })),
    ])
    .await;
    let mut subscription = subscribe(base).await;
    let error = subscription
        .recv()
        .await
        .expect("an error, not a quiet end")
        .expect_err("the session's end is the stream's end");
    assert_eq!(
        error.stream_end_reason(),
        Some(StreamEndReason::SessionEnded)
    );
    assert!(subscription.recv().await.is_none());
}

#[tokio::test]
async fn a_stream_frame_still_wins_when_it_arrives_first() {
    let end = StreamEndNotice::new(StreamEndReason::DeviceRevoked).into_error();
    let base = serve(vec![binary(&ServerFrame::err(1, &end))]).await;
    let mut subscription = subscribe(base).await;
    let error = subscription.recv().await.unwrap().unwrap_err();
    assert_eq!(
        error.stream_end_reason(),
        Some(StreamEndReason::DeviceRevoked)
    );
}
