//! Idempotency-key middleware.
//!
//! Protects mutating routes against duplicate execution. On the first request
//! with a given `Idempotency-Key`, the handler runs and the captured response
//! is persisted. Subsequent requests with the same key replay the stored
//! response if the request body hashes match, or return `422` with a
//! `idempotency_key_conflict` code if a different body is sent under the same
//! key (per the draft IETF spec).
//!
//! Usage:
//! ```text
//! use cratestack_axum::idempotency::{IdempotencyLayer, SqlxIdempotencyStore};
//! use std::net::SocketAddr;
//! let store = std::sync::Arc::new(SqlxIdempotencyStore::new(pool.clone()));
//! let router = generated_router.layer(IdempotencyLayer::new(store, std::time::Duration::from_secs(24 * 3600)));
//!
//! // The default principal fingerprint hashes `Authorization` when present
//! // and otherwise falls back to the verified TCP peer address, which
//! // axum only populates via `ConnectInfo<SocketAddr>` when the server is
//! // served through `into_make_service_with_connect_info`:
//! let listener = tokio::net::TcpListener::bind(addr).await?;
//! axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>()).await?;
//! ```
//!
//! **This wiring matters.** Nothing in this crate — and, as of this
//! writing, no example shipped in this repository — serves through
//! `into_make_service_with_connect_info` by default; every example uses
//! plain `into_make_service()`. Without it, `ConnectInfo<SocketAddr>` is
//! never present in request extensions, so *every* request without an
//! `Authorization` header is refused with `412 Precondition Failed`
//! (cratestack#416 — the default used to silently collapse such requests
//! onto a shared `"anonymous"` namespace instead; it now refuses rather
//! than risk that collision). Consumers who authenticate via cookies/mTLS
//! rather than an `Authorization` header — and who cannot serve through
//! `into_make_service_with_connect_info` — must supply
//! [`IdempotencyLayer::with_principal_fingerprint`] explicitly.
//!
//! In Phase 1 the layer is opt-in at the consumer's router. A follow-up will
//! wire it into macro-generated routers by default, gated by a
//! `@no_idempotency` opt-out attribute already recognised by the parser.

mod complete;
mod hash;
mod headers;
mod layer;
mod parse;
mod record;
mod responses;
mod service;
mod store;
mod stream_bypass;

#[cfg(test)]
mod tests_fingerprint;
#[cfg(test)]
mod tests_hash;
#[cfg(test)]
mod tests_headers;
#[cfg(test)]
mod tests_parse;
#[cfg(test)]
mod tests_stream_bypass;

pub use hash::{hash_request, is_idempotent_target_method};
pub use headers::{decode_headers, encode_headers};
pub use layer::IdempotencyLayer;
pub use parse::parse_idempotency_key;
pub use record::{IdempotencyRecord, ReservationOutcome};
pub use service::IdempotencyService;
pub use store::{IDEMPOTENCY_TABLE_DDL, IdempotencyStore};
