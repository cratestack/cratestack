//! The socket's reads, writes and clocks.

use axum::extract::ws::{Message, WebSocket};
use cratestack_core::subscription_wire::{ClientFrame, ServerFrame};
use cratestack_core::{CratestackContext, CratestackError};
use futures_util::{SinkExt, StreamExt};
use tokio::time::Instant;

use crate::close::Sink;
use crate::codec::WireCodec;

pub(super) async fn next_frame(
    stream: &mut futures_util::stream::SplitStream<WebSocket>,
    codec: WireCodec,
) -> Option<Result<ClientFrame, CratestackError>> {
    loop {
        match stream.next().await? {
            Ok(Message::Ping(_) | Message::Pong(_)) => continue,
            Ok(message @ (Message::Text(_) | Message::Binary(_))) => {
                return Some(codec.decode(&message));
            }
            Ok(Message::Close(_)) | Err(_) => return None,
        }
    }
}

pub(super) async fn send(sink: &mut Sink, codec: WireCodec, frame: &ServerFrame) -> Result<(), ()> {
    let message = codec.encode(frame).map_err(|error| {
        tracing::warn!(target: "cratestack", %error, "WebSocket frame failed to encode");
    })?;
    sink.send(message).await.map_err(|_| ())
}

/// When the context's `exp` claim passes, if it has one.
pub(super) fn expiry_of(ctx: &CratestackContext) -> Option<Instant> {
    let exp = ctx.expires_at()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    Some(Instant::now() + std::time::Duration::from_secs(exp.saturating_sub(now).max(0) as u64))
}

pub(super) async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}
