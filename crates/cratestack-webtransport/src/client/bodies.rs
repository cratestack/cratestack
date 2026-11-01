//! The error bodies a client reports for a session or stream that ended
//! without an `err` frame: what the close code or a reset means.

use cratestack_core::rpc::RpcErrorBody;

/// The server dropped the stream or the session: resubscribe and refetch.
pub(super) fn going_away() -> RpcErrorBody {
    body("unavailable", "the WebTransport session went away")
}

pub(super) fn closed_body(code: u32) -> RpcErrorBody {
    use crate::frame::close_code;
    match code {
        close_code::UNAUTHENTICATED => body("unauthenticated", "the session was refused"),
        close_code::FORBIDDEN => body("permission_denied", "the session was refused"),
        close_code::TOO_MANY_SESSIONS => body("resource_exhausted", "too many sessions"),
        _ => body("unavailable", &format!("the session closed with {code}")),
    }
}

fn body(code: &str, message: &str) -> RpcErrorBody {
    RpcErrorBody {
        code: code.to_owned(),
        message: message.to_owned(),
        ..RpcErrorBody::default()
    }
}
