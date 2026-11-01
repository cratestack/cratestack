//! Ending a socket (design §6.3): a refusal is never just a close code.
//! The `{id: 0, err: RpcErrorBody}` frame goes first, so an `AuthRefusal`'s
//! `details` and extra members reach the client as on REST and RPC, then
//! the close: 4401, 4403, 4400 or 4500 (a provider that could not decide:
//! retry, do not sign out).

use axum::extract::ws::{CloseFrame, Message, WebSocket};
use cratestack_core::CratestackError;
use cratestack_core::subscription_wire::{ServerFrame, refusal_close_code};
use futures_util::SinkExt;
use futures_util::stream::SplitSink;

use crate::codec::WireCodec;

pub(crate) type Sink = SplitSink<WebSocket, Message>;

/// WebSocket close codes this binding sends besides the session codes
/// (`cratestack_core::subscription_wire::close_code`).
pub mod ws_close {
    /// The peer stopped answering pings, or the server failed.
    pub const GOING_AWAY: u16 = 1001;
    /// The client sent a frame this socket cannot accept.
    pub const UNSUPPORTED: u16 = 1003;
    /// The client is too slow to drain its frames (try again later).
    pub const TRY_AGAIN_LATER: u16 = 1013;
}

pub(crate) async fn close(sink: &mut Sink, code: u16, reason: &'static str) {
    tracing::Span::current().record("close", code);
    let frame = CloseFrame {
        code,
        reason: reason.into(),
    };
    let _ = sink.send(Message::Close(Some(frame))).await;
}

/// Sends the refusal's `{id: 0, err}` frame, then closes with its code.
pub(crate) async fn refuse(
    sink: &mut Sink,
    codec: WireCodec,
    error: &CratestackError,
    reason: &'static str,
) {
    let code = refusal_close_code(error) as u16;
    tracing::info!(target: "cratestack", close = code, error = %error, "WebSocket session refused");
    if let Ok(frame) = codec.encode(&ServerFrame::err(0, error)) {
        let _ = sink.send(frame).await;
    }
    close(sink, code, reason).await;
}
