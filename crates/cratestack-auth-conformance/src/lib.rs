//! The `AuthProvider` conformance kit (ADR 0021 §6, cratestack#1243).
//!
//! A provider author, in-house or external, writes one test:
//!
//! ```text
//! #[tokio::test]
//! async fn my_provider_conforms() {
//!     cratestack_auth_conformance::auth_conformance(MyProvider::new(keys))
//!         .accepts("user-42", |canonical| sign(canonical))
//!         .refuses("expired", |canonical| expired(canonical), Expected::refusal(&refusal))
//!         .run()
//!         .await;
//! }
//! ```
//!
//! The kit compiles its own small schemas and drives the provider through
//! REST, RPC unary and RPC batch (each mounted at the root and under
//! `/kit`), and MCP Streamable HTTP with
//! `CredentialPresence::ask_provider()`. For every case it asserts:
//!
//! - **the same verdict on every transport**: an accepted credential yields
//!   the same subject, and a refused one the same refusal;
//! - **the same rendering**: status, code (REST/MCP or RPC), message,
//!   envelope members, `details` and `WWW-Authenticate` challenges (MCP's
//!   own `Bearer` challenge first, then the provider's);
//! - **the canonical request** of ADR 0021 §3 exactly: method, path without
//!   the mount prefix, query, and the exact body bytes;
//! - **one call per request**: a batch is authenticated once, and a request
//!   whose credential the provider calls absent never reaches it on MCP.
//!
//! A signer is handed the [`Canonical`] request before it is sent, so a
//! signing provider can sign exactly what it will be given.
//!
//! **Not driven yet:** the SSE model subscriptions and the sync endpoints,
//! which need a database, and WebTransport, which is not released. A
//! transport the kit does not drive is a transport where parity is unproven.

mod apps;
mod canonical;
mod check;
mod credentials;
mod expect;
mod run;
mod send;

pub use canonical::{Canonical, Transport};
pub use credentials::Credentials;
pub use expect::Expected;
pub use run::{Conformance, auth_conformance};
