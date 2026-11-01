//! The WebSocket subscription binding
//! (`docs/design/subscriptions-websocket.md` §6).
//!
//! One socket per client, `GET /rpc/ws` (`/$ws` on REST schemas), speaking
//! the frames the WebTransport binding speaks: `Init`, then `Request`s
//! multiplexed by id, each answered by `Ready`, items and one `end` or
//! `err`; `Cancel` ends one. Binary messages carry CBOR
//! (`cratestack.rpc.v1+cbor`), text messages JSON (`cratestack.rpc.v1+json`).
//! It is the fallback where UDP is blocked and WebTransport cannot run.
//!
//! This crate only moves frames. Opening a subscription — policy, setup,
//! the event source — is a [`cratestack_core::SubscriptionDispatcher`],
//! which the schema macro implements once for every binding, and
//! authentication is the same [`cratestack_core::AuthProvider`] the HTTP
//! router uses: over the upgrade request's headers, or over `Init`'s
//! headers for a browser, whose `WebSocket` cannot set any.
//!
//! ```no_run
//! # use cratestack_websocket::{OriginPolicy, WebSocketConfig};
//! # fn run<A: cratestack_core::AuthProvider, D: cratestack_core::SubscriptionDispatcher>(
//! #     auth: A, dispatcher: D, app: axum::Router,
//! # ) -> axum::Router {
//! let config = WebSocketConfig::new()
//!     .origins(OriginPolicy::Allow(vec!["https://app.example.com".into()]));
//! app.merge(cratestack_websocket::router(config, auth, dispatcher))
//! # }
//! ```

mod close;
mod codec;
mod config;
mod server;
mod session;
mod subscription;

pub use close::ws_close;
pub use codec::{CBOR_PROTOCOL, JSON_PROTOCOL, WireCodec};
pub use config::{PrincipalKeyFn, WebSocketConfig, WebSocketLimits};
pub use cratestack_core::subscription_wire::{
    ClientFrame, ClientMessage, Empty, InitBody, OriginPolicy, ServerFrame, close_code,
    default_principal_key,
};
pub use server::router;
