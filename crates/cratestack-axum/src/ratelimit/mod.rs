//! Per-principal rate limiting.
//!
//! Token-bucket algorithm with a pluggable store. The default in-memory
//! implementation is appropriate for single-instance deployments; banks
//! running multiple replicas bring a Redis-backed implementation through
//! the [`RateLimitStore`] trait so all replicas share the same view of
//! consumption.
//!
//! The middleware computes a key per request (the default hashes the
//! `Authorization` header when present and otherwise falls back to the
//! verified TCP peer address, the same shape the idempotency layer uses)
//! and refuses with `429` plus a `Retry-After` header when the bucket is
//! empty. Banks running tenant-scoped budgeting can swap the key function
//! for tenant-id.
//!
//! Usage:
//! ```ignore
//! use cratestack_axum::ratelimit::{InMemoryRateLimitStore, RateLimitConfig, RateLimitLayer};
//! use std::net::SocketAddr;
//! let store = std::sync::Arc::new(InMemoryRateLimitStore::default());
//! let router = generated_router.layer(RateLimitLayer::new(store, RateLimitConfig::new(100, 1.0)));
//!
//! // The peer-address fallback below only kicks in when the server is
//! // served through `into_make_service_with_connect_info`:
//! let listener = tokio::net::TcpListener::bind(addr).await?;
//! axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>()).await?;
//! ```
//!
//! **This wiring matters.** Nothing in this crate — and, as of this
//! writing, no example shipped in this repository — serves through
//! `into_make_service_with_connect_info` by default; every example uses
//! plain `into_make_service()`. Without it, `ConnectInfo<SocketAddr>` is
//! never present in request extensions, so *every* caller without an
//! `Authorization` header shares a single `"anonymous"` bucket —
//! effectively no per-caller throttling for that traffic. Consumers who
//! authenticate via cookies/mTLS rather than an `Authorization` header —
//! and who cannot serve through `into_make_service_with_connect_info` —
//! must supply [`RateLimitLayer::with_key_fn`] explicitly; relying on the
//! default alone does not, by itself, separate such callers.

mod config;
mod layer;
mod store;

pub use config::{_bucket_capacity_for, RateLimitConfig, RateLimitDecision};
pub use layer::{RateLimitLayer, RateLimitService};
pub use store::{InMemoryRateLimitStore, RateLimitStore};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_key_fn;
