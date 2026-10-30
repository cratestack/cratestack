//! The WebTransport subscription binding
//! (`docs/design/subscriptions-websocket.md` §6a).
//!
//! One HTTP/3 WebTransport session per client; inside it, a control stream
//! for `Init` (and later re-authentication) and **one client-opened
//! bidirectional QUIC stream per subscription**. Streams do not hold each
//! other up, a client resetting a stream is its cancel, and the session
//! survives the client changing networks. The frames, the `Init`/`Ready`
//! handshake and the delivery contract are the ones the SSE and WebSocket
//! bindings use.
//!
//! This crate only moves frames. Opening a subscription — policy, setup,
//! the event source — is a [`cratestack_core::SubscriptionDispatcher`],
//! which the schema macro implements once for every binding, and
//! authentication is the same [`cratestack_core::AuthProvider`] the HTTP
//! router uses.
//!
//! ```no_run
//! # use cratestack_webtransport::{OriginPolicy, WebTransportConfig, WebTransportServer};
//! # async fn run<A: cratestack_core::AuthProvider, D: cratestack_core::SubscriptionDispatcher>(
//! #     auth: A, dispatcher: D,
//! #     chain: Vec<cratestack_webtransport::CertificateDer<'static>>,
//! #     key: cratestack_webtransport::PrivateKeyDer<'static>,
//! # ) -> Result<(), cratestack_core::CratestackError> {
//! let config = WebTransportConfig::new("[::]:443".parse().unwrap(), chain, key)
//!     .origins(OriginPolicy::Allow(vec!["https://app.example.com".into()]));
//! WebTransportServer::bind(config, auth, dispatcher)?.serve().await;
//! # Ok(()) }
//! ```
//!
//! Axum and hyper do not serve HTTP/3, so this is a second listener — UDP,
//! normally on the same port number as HTTPS — in the same process. What
//! that asks of a deployment (an L4 UDP load balancer, connection-ID
//! routing to keep sessions across network changes, a WebSocket fallback
//! for networks that block UDP) is in the design doc, §6a.6.

mod admission;
mod codec;
mod config;
#[cfg(feature = "dev-cert")]
mod dev_cert;
mod frame;
mod origin;
mod principal;
mod reader;
mod server;
mod session;
mod subscription;
mod transport;

pub use codec::{CBOR_PROTOCOL, FrameDecoder, JSON_PROTOCOL, WireCodec};
pub use config::{PrincipalKeyFn, WebTransportConfig, WebTransportLimits};
#[cfg(feature = "dev-cert")]
pub use dev_cert::DevCertificate;
pub use frame::{ClientFrame, ClientMessage, Empty, InitBody, ServerFrame, close_code, reset_code};
pub use origin::OriginPolicy;
pub use principal::default_principal_key;
pub use server::WebTransportServer;
pub use transport::{CertificateDer, PrivateKeyDer};
