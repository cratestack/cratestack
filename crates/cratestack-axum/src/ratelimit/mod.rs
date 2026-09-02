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
//! ## Store failures fail OPEN by default (cratestack#846)
//!
//! Identity derivation above is fail-closed because its inputs are
//! caller-controlled. A failure of the *store* is not: when Redis drops a
//! connection, no caller caused it and no caller can fix it, and refusing
//! would turn a limiter hiccup into a simultaneous outage of every
//! rate-limited route. The layer therefore logs a `WARN` and lets the
//! request through — the limiter protects capacity, so a broken limiter
//! degrades to unlimited. Deployments using the limiter as a security
//! control (a paywall, a brute-force guard) opt into the opposite with
//! [`RateLimitLayer::with_store_error_policy`]/[`StoreErrorPolicy::Deny`].
//!
//! Every response the layer emits itself — the throttled `429`, an
//! identity refusal, a `Deny`d store failure — carries the framework's
//! own codec-negotiated error envelope, so a generated client decodes a
//! typed code rather than an opaque body.

mod config;
mod key_fn;
mod layer;
mod policy;
mod rest_ops_filter;
mod rpc_ops_filter;
mod store;

pub use config::{_bucket_capacity_for, RateLimitConfig, RateLimitDecision};
pub use layer::{RateLimitLayer, RateLimitService};
pub use policy::StoreErrorPolicy;
pub use rest_ops_filter::build_rest_ops_filter;
pub use rpc_ops_filter::build_rpc_ops_filter;
pub use store::{InMemoryRateLimitStore, RateLimitStore};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_store_error;
#[cfg(test)]
mod tests_support;
#[cfg(test)]
mod tests_key_fn;
