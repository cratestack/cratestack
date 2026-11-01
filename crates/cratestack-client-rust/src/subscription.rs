//! Subscriptions (`docs/design/subscriptions-websocket.md` §8): one
//! contract whatever the binding — the call resolves once the server says
//! `Ready`, then the stream yields items and ends, or ends with an error.
//!
//! The binding is client configuration ([`SubscriptionTransport`]), never
//! part of a generated method's name. `Auto` uses a WebSocket when the `ws`
//! feature is on (native targets) and falls back to SSE when the socket
//! cannot be opened; a refusal is never a reason to fall back.

mod frames;
mod sse;
#[cfg(all(feature = "ws", not(target_arch = "wasm32")))]
mod ws;

use serde::de::DeserializeOwned;

use crate::client::CratestackClient;
use crate::codec::HttpClientCodec;
use crate::error::ClientError;

pub use frames::rpc_error_to_client;

/// A live subscription: items until the server ends it, or a terminal `Err`.
/// `None` after an `Err`, or when the stream ends normally (a subscription
/// procedure whose source ran out). Dropping it cancels the subscription.
pub type ClientSubscription<T> = tokio::sync::mpsc::Receiver<Result<T, ClientError>>;

/// Which binding a subscription uses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SubscriptionTransport {
    /// A WebSocket when this build has the `ws` feature, else SSE; SSE as
    /// well when the socket cannot be opened.
    #[default]
    Auto,
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

impl<C: HttpClientCodec> CratestackClient<C> {
    /// Opens `target`, resolving once the server says `Ready`.
    pub async fn subscribe<T>(
        &self,
        target: SubscriptionTarget,
    ) -> Result<ClientSubscription<T>, ClientError>
    where
        T: DeserializeOwned + Send + 'static,
    {
        match self.config.subscription_transport {
            SubscriptionTransport::Sse => sse::open(self, &target).await,
            SubscriptionTransport::WebSocket => self.subscribe_ws(&target).await,
            SubscriptionTransport::Auto => self.subscribe_auto(&target).await,
        }
    }

    #[cfg(all(feature = "ws", not(target_arch = "wasm32")))]
    async fn subscribe_ws<T: DeserializeOwned + Send + 'static>(
        &self,
        target: &SubscriptionTarget,
    ) -> Result<ClientSubscription<T>, ClientError> {
        ws::open(self, target)
            .await
            .map_err(ws::Failure::into_error)
    }

    #[cfg(not(all(feature = "ws", not(target_arch = "wasm32"))))]
    async fn subscribe_ws<T: DeserializeOwned + Send + 'static>(
        &self,
        _target: &SubscriptionTarget,
    ) -> Result<ClientSubscription<T>, ClientError> {
        Err(ClientError::BadInput(
            "WebSocket subscriptions need the `ws` feature on a native target".to_owned(),
        ))
    }

    #[cfg(all(feature = "ws", not(target_arch = "wasm32")))]
    async fn subscribe_auto<T: DeserializeOwned + Send + 'static>(
        &self,
        target: &SubscriptionTarget,
    ) -> Result<ClientSubscription<T>, ClientError> {
        match ws::open(self, target).await {
            Ok(subscription) => Ok(subscription),
            // The socket could not be opened (no route, a proxy that blocks
            // upgrades): SSE. A refusal is the server's answer, not a
            // transport failure, and is returned as is.
            Err(ws::Failure::Unreachable(_)) => sse::open(self, target).await,
            Err(failure) => Err(failure.into_error()),
        }
    }

    #[cfg(not(all(feature = "ws", not(target_arch = "wasm32"))))]
    async fn subscribe_auto<T: DeserializeOwned + Send + 'static>(
        &self,
        target: &SubscriptionTarget,
    ) -> Result<ClientSubscription<T>, ClientError> {
        sse::open(self, target).await
    }
}
