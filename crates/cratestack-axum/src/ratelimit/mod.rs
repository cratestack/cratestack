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
//! ```text
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
//! never present in request extensions, so *every* request without an
//! `Authorization` header is refused with `412 Precondition Failed`
//! (cratestack#416 — the default used to silently collapse such requests
//! onto a shared `"anonymous"` bucket instead, which meant no per-caller
//! throttling at all for that traffic; it now refuses rather than risk
//! that collision). Consumers who authenticate via cookies/mTLS rather
//! than an `Authorization` header — and who cannot serve through
//! `into_make_service_with_connect_info` — must supply
//! [`RateLimitLayer::with_key_fn`] explicitly.
//!
//! ## Store failures fail open ONLY when they are transport-class (cratestack#846)
//!
//! Identity derivation above is fail-closed because its inputs are
//! caller-controlled. A *transport* failure of the store is not: when
//! Redis drops a connection, no caller caused it, no caller can fix it,
//! and it self-heals — so refusing would turn a limiter hiccup into a
//! simultaneous outage of every rate-limited route. The layer logs a
//! `WARN` and lets those through.
//!
//! A store that is *reachable and refusing* is a different animal, and
//! the reason this is not a blanket fail-open: an `OOM` is inducible by
//! any unauthenticated caller, because the default key function above
//! hashes an unvalidated `Authorization` header. Those failures stay
//! closed under every policy. See [`StoreErrorPolicy`] for the full
//! argument. Deployments using the limiter as a security control opt into
//! refusing even transport failures with
//! [`RateLimitLayer::with_store_error_policy`]/[`StoreErrorPolicy::Deny`],
//! and [`RateLimitLayer::with_store_timeout`] bounds how long a lookup
//! may block before the policy applies at all.
//!
//! Every response the layer emits itself — the throttled `429`, an
//! identity refusal, a `Deny`d store failure — carries the framework's
//! own codec-negotiated error envelope, so a generated client decodes a
//! typed code rather than an opaque body.

mod config;
mod decision;
mod key_fn;
mod layer;
mod policy;
mod rest_ops_filter;
mod rpc_ops_filter;
mod store;
mod store_error;

pub use config::{_bucket_capacity_for, RateLimitConfig, RateLimitDecision};
pub use layer::{RateLimitLayer, RateLimitService};
pub use policy::{DEFAULT_STORE_TIMEOUT, StoreErrorPolicy};
pub use rest_ops_filter::build_rest_ops_filter;
pub use rpc_ops_filter::build_rpc_ops_filter;
pub use store::{InMemoryRateLimitStore, RateLimitStore};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_key_fn;
#[cfg(test)]
mod tests_store_error;
#[cfg(test)]
mod tests_store_timeout;
#[cfg(test)]
mod tests_typed_bodies;
#[cfg(test)]
mod tests_support;
