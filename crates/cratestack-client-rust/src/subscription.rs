//! Subscriptions (`docs/design/subscriptions-websocket.md` §8): one
//! contract whatever the binding — the call resolves once the server says
//! `Ready`, then the stream yields items and ends, or ends with an error.
//!
//! The binding is client configuration ([`SubscriptionTransport`]), never
//! part of a generated method's name. `Auto` tries WebTransport when the
//! `webtransport` feature is on and the config names the listener, then a
//! WebSocket when the `ws` feature is on, then SSE (native targets for the
//! first two). It moves on only when a binding cannot be opened; a refusal
//! is never a reason to fall back.

mod failure;
mod frames;
mod open;
mod sse;
#[cfg(all(feature = "ws", not(target_arch = "wasm32")))]
mod ws;
#[cfg(all(feature = "webtransport", not(target_arch = "wasm32")))]
mod wt;

pub use frames::rpc_error_to_client;

/// A live subscription: items until the server ends it, or a terminal `Err`.
/// `None` after an `Err`, or when the stream ends normally (a subscription
/// procedure whose source ran out). Dropping it cancels the subscription.
pub type ClientSubscription<T> = tokio::sync::mpsc::Receiver<Result<T, ClientError>>;

use crate::error::ClientError;

/// Which binding a subscription uses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SubscriptionTransport {
    /// WebTransport, then a WebSocket, then SSE: each binding this build
    /// has and the config allows, moving on only when one cannot be opened.
    #[default]
    Auto,
    /// One WebTransport session per subscription (`webtransport` feature,
    /// native targets, `ClientConfig::with_webtransport`).
    WebTransport,
    /// One WebSocket per subscription (`ws` feature, native targets).
    WebSocket,
    /// `text/event-stream` over HTTP.
    Sse,
}

/// Where a subscription is served. The generated client fills it in; the
/// binding decides which half it reads.
#[derive(Debug, Clone)]
pub struct SubscriptionTarget {
    /// `model.<Model>.subscribe` or `procedure.<name>`.
    pub op_id: String,
    /// The input a socket's `Request` carries: a model feed's
    /// `RpcSubscribeInput`, a subscription procedure's `Args`.
    pub input: serde_json::Value,
    /// The SSE route: `/rpc/subscribe/<op_id>` on `transport rpc`,
    /// `/<plural>/$subscribe` or `/$procs/<name>` on REST.
    pub sse_path: String,
    /// The input as that route's query string.
    pub sse_query: Option<String>,
    /// The socket path: `/rpc/ws`, or `/$ws` on REST.
    pub socket_path: &'static str,
}

impl SubscriptionTarget {
    /// A subscription procedure's `Args` as its SSE query string: each
    /// argument a parameter whose value is that argument's JSON.
    pub fn procedure_query(args: &serde_json::Value) -> Option<String> {
        let object = args.as_object().filter(|object| !object.is_empty())?;
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        for (name, value) in object {
            query.append_pair(name, &value.to_string());
        }
        Some(query.finish())
    }
}
