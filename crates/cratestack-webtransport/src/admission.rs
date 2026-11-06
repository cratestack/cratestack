//! Authenticating a session, and refusing one (design §6a.4; ADR 0021 §1,
//! cratestack-docs `internals/auth-seam-adr.md`).
//!
//! A refusal is never just a close code. The server first writes
//! `{id: 0, err: RpcErrorBody}` on the control stream — `Init` is id 0 and
//! `Ready{0}` acknowledges it, so the frame already belongs to the protocol
//! — then closes the session. The body comes from
//! `RpcErrorBody::from_cratestack`, so an `AuthRefusal`'s `details` and
//! extra members (the `error` reason a device-signature client switches on)
//! reach the client the same way they do on REST and RPC. Its
//! `WWW-Authenticate` challenges do not: a frame has no headers, and
//! ADR 0021 Q5 keeps them out of the body for now.

use std::time::Duration;

use cratestack_core::CratestackError;

use crate::codec::WireCodec;
use crate::frame::ServerFrame;
use crate::transport::SendHalf;

/// How long a refusal waits for the client to acknowledge its `err` frame
/// before closing anyway.
const REFUSAL_GRACE: Duration = Duration::from_secs(2);

pub(crate) use cratestack_core::subscription_wire::{
    Handshake as Connect, authenticate_session as authenticate, init_headers, refusal_close_code,
};

/// Writes the refusal's `{id: 0, err}` frame, finishes the control stream
/// and waits — bounded — until the client has acknowledged it, so the
/// session close that follows cannot overtake the frame. Returns the close
/// code and reason for the caller to close with.
pub(crate) async fn refuse(
    control: &mut SendHalf,
    codec: WireCodec,
    error: &CratestackError,
    reason: &'static str,
) -> (u32, &'static str) {
    let code = refusal_close_code(error);
    tracing::info!(target: "cratestack", close = code, error = %error, "WebTransport session refused");
    if let Ok(frame) = codec.encode(&ServerFrame::err(0, error))
        && control.write_all(&frame).await.is_ok()
    {
        control.finish();
        let _ = tokio::time::timeout(REFUSAL_GRACE, control.stopped()).await;
    }
    (code, reason)
}
