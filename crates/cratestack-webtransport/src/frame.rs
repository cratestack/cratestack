//! The frames a WebTransport subscription session carries
//! (`docs/design/subscriptions-websocket.md` §6.4, §6a.2).
//!
//! Field names follow what already ships rather than the older §2.3
//! sketch: a request is the batch binding's `RpcRequest` shape (`id`, `op`,
//! `input`), and items/errors use the SSE binding's `next`/`err`
//! (`cratestack-axum/src/rpc/sse.rs`). A frame's kind is whichever of
//! `init`/`op`/`ready`/`next`/`end`/`err` is present — there is no separate
//! type tag, so new kinds are additive.

use std::collections::BTreeMap;

use cratestack_core::CratestackError;
use cratestack_core::rpc::RpcErrorBody;
use serde::{Deserialize, Serialize};

/// Session close codes (sent with the WebTransport session close).
pub mod close_code {
    /// The control stream broke protocol: first frame not `Init`, a frame
    /// that does not decode, or a frame over the size limit.
    pub const PROTOCOL: u32 = 4400;
    /// Authentication was refused (401), or no `Init` arrived in time.
    /// A refusal is preceded by an `{id: 0, err}` frame; a timeout is not.
    pub const UNAUTHENTICATED: u32 = 4401;
    /// A 403 refusal: the provider forbade the caller, or a
    /// re-authentication named a different principal. The `{id: 0, err}`
    /// frame before the close says which.
    pub const FORBIDDEN: u32 = 4403;
    /// Too many open sessions for this principal.
    pub const TOO_MANY_SESSIONS: u32 = 4429;
    /// The `AuthProvider` could not decide (a 5xx: its key server is down).
    /// Not a verdict on the credentials: retry later, do not sign out.
    pub const SERVER_ERROR: u32 = 4500;
}

/// Stream reset codes (sent on one subscription's stream).
pub mod reset_code {
    /// The session is shutting down; treat the subscription as
    /// `unavailable` and resubscribe.
    pub const GOING_AWAY: u32 = 0x01;
    /// The stream broke protocol: not exactly one `Request`, or an `id`
    /// that does not match it.
    pub const PROTOCOL: u32 = 0x02;
}

/// An empty object (`{}`), the payload of `ready` and `end`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Empty {}

/// `Init` body: the credentials a browser cannot put on the CONNECT
/// request. Empty for a client that authenticated with CONNECT headers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitBody {
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

/// Anything a client sends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClientFrame {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub init: Option<InitBody>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<serde_json::Value>,
}

/// A client frame, classified.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientMessage {
    Init(InitBody),
    Request {
        id: u64,
        op: String,
        input: serde_json::Value,
    },
}

impl ClientFrame {
    pub fn init(headers: BTreeMap<String, String>) -> Self {
        Self {
            id: 0,
            init: Some(InitBody { headers }),
            op: None,
            input: None,
        }
    }

    pub fn request(id: u64, op: impl Into<String>, input: serde_json::Value) -> Self {
        Self {
            id,
            init: None,
            op: Some(op.into()),
            input: Some(input),
        }
    }

    /// Exactly one of `init` / `op` must be present; `Init` is always id 0.
    pub fn classify(self) -> Result<ClientMessage, CratestackError> {
        match (self.init, self.op) {
            (Some(init), None) if self.id == 0 => Ok(ClientMessage::Init(init)),
            (Some(_), None) => Err(bad_frame("an `init` frame must have id 0")),
            (None, Some(op)) => Ok(ClientMessage::Request {
                id: self.id,
                op,
                input: self.input.unwrap_or(serde_json::Value::Null),
            }),
            (Some(_), Some(_)) => Err(bad_frame("a frame carries `init` or `op`, not both")),
            (None, None) => Err(bad_frame("a client frame needs `init` or `op`")),
        }
    }
}

/// Anything the server sends. Exactly one of the optional fields is set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerFrame {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ready: Option<Empty>,
    /// Present-but-`null` is an item whose value is `null`, not "no item":
    /// the kind is decided by which key is present (module docs).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub next: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<Empty>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub err: Option<RpcErrorBody>,
}

impl ServerFrame {
    fn empty(id: u64) -> Self {
        Self {
            id,
            ready: None,
            next: None,
            end: None,
            err: None,
        }
    }

    pub fn ready(id: u64) -> Self {
        Self {
            ready: Some(Empty {}),
            ..Self::empty(id)
        }
    }

    pub fn next(id: u64, item: serde_json::Value) -> Self {
        Self {
            next: Some(item),
            ..Self::empty(id)
        }
    }

    pub fn end(id: u64) -> Self {
        Self {
            end: Some(Empty {}),
            ..Self::empty(id)
        }
    }

    pub fn err(id: u64, error: &CratestackError) -> Self {
        Self {
            err: Some(RpcErrorBody::from_cratestack(error)),
            ..Self::empty(id)
        }
    }
}

fn present<'de, D>(deserializer: D) -> Result<Option<serde_json::Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    serde_json::Value::deserialize(deserializer).map(Some)
}

fn bad_frame(message: &str) -> CratestackError {
    CratestackError::BadRequest(format!("invalid WebTransport frame: {message}"))
}
