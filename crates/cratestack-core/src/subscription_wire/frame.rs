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

use crate::CratestackError;
use crate::rpc::RpcErrorBody;
use serde::{Deserialize, Serialize};

pub use super::codes::{close_code, reset_code};

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
    /// Ends subscription `id` (WebSocket; on WebTransport, resetting the
    /// stream is the cancel).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel: Option<Empty>,
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
    Cancel {
        id: u64,
    },
}

impl ClientFrame {
    pub fn init(headers: BTreeMap<String, String>) -> Self {
        Self {
            id: 0,
            init: Some(InitBody { headers }),
            op: None,
            input: None,
            cancel: None,
        }
    }

    pub fn request(id: u64, op: impl Into<String>, input: serde_json::Value) -> Self {
        Self {
            id,
            init: None,
            op: Some(op.into()),
            input: Some(input),
            cancel: None,
        }
    }

    pub fn cancel(id: u64) -> Self {
        Self {
            id,
            init: None,
            op: None,
            input: None,
            cancel: Some(Empty {}),
        }
    }

    /// Exactly one of `init` / `op` / `cancel` must be present; `Init` is
    /// always id 0.
    pub fn classify(self) -> Result<ClientMessage, CratestackError> {
        if self.cancel.is_some() {
            return if self.init.is_none() && self.op.is_none() {
                Ok(ClientMessage::Cancel { id: self.id })
            } else {
                Err(bad_frame("a `cancel` frame carries nothing else"))
            };
        }
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
    CratestackError::BadRequest(format!("invalid subscription frame: {message}"))
}
