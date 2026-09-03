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
//!
//! ## The bucket keyspace is bounded (cratestack#871)
//!
//! The `auth:` key above is a hash of an **unverified** header, because
//! this layer runs before authentication. Left alone, that is an
//! amplification primitive: rotate the header and mint one store key per
//! request. It is what made the `OOM` case in the previous section
//! reachable by anyone in the first place.
//!
//! So the default derivation no longer returns a bare key. It returns the
//! key *plus* a [`cratestack_core::BucketBudget`] naming the scope that
//! key is counted against, how many distinct buckets that scope may hold
//! at once, and which bucket to charge instead once it is full:
//!
//! | Request carries | Key | Scope | Cap | Fallback |
//! |---|---|---|---|---|
//! | [`VerifiedPrincipal`] extension | `princ:<sha256>` | — | none | — |
//! | `Authorization` + `ConnectInfo` | `auth:<sha256>` | `peer:<addr>` | 128 | `ip:<addr>` |
//! | `Authorization`, no `ConnectInfo` | `auth:<sha256>` | `global` | 8192 | `overflow` |
//! | `ConnectInfo` only | `ip:<addr>` | — | none | — |
//! | neither | *refused, `412`* (cratestack#416) | | | |
//!
//! `<addr>` is the peer address for IPv4 and its **/64 prefix** for IPv6,
//! in the scope AND in every bucket key. Aggregating only the scope left
//! the whole mechanism evadable by rotating the source address inside one
//! subscriber prefix — measured at 200 buckets with a token and 200
//! buckets, all allowed, without one.
//!
//! The store applies the budget atomically alongside the token
//! consumption — doing it as a separate lookup would race, with N
//! concurrent requests each reading "under budget" and each minting a
//! bucket.
//!
//! Each admitted credential holds a **slot** that expires
//! [`cratestack_core::scope_ttl_secs`] after it was last used — at least
//! the buckets' own TTL, so a slot always outlives the bucket it admitted
//! and no fresh generation can open beneath a live one. The window slides
//! per credential: an actively-used caller never loses its slot, and a
//! peer whose tokens rotate reclaims the slots of credentials it stopped
//! using. `window` (default 60s) is the *floor* on that lifetime, not a
//! fixed period that resets the scope. Keyspace is O(peers × cap) at every
//! instant.
//!
//! Over the cap the caller is *collapsed onto its own* `ip:` bucket, not
//! refused: refusing would hand an attacker a deterministic outage of
//! every rate-limited route, which is the failure mode cratestack#846 was
//! fought over. Under the cap, distinct callers still never share
//! (cratestack#416). Tune with
//! [`RateLimitLayer::with_bucket_budget`], opt out with
//! [`RateLimitLayer::without_bucket_budget`], and see
//! [`UnverifiedAuthPolicy`] for the stronger "ignore the header entirely"
//! mode. `docs/design/ratelimit-bucket-cardinality.md` states what is
//! **not** bounded.

mod budget;
mod config;
mod consume;
mod decision;
mod key_fn;
mod layer;
mod policy;
mod rest_ops_filter;
mod rpc_ops_filter;
mod scope;
mod service;
mod store;
mod store_error;

pub use budget::RateLimitBucketBudget;
pub use config::{_bucket_capacity_for, RateLimitConfig, RateLimitDecision};
pub use layer::RateLimitLayer;
pub use policy::{DEFAULT_STORE_TIMEOUT, StoreErrorPolicy};
pub use rest_ops_filter::build_rest_ops_filter;
pub use rpc_ops_filter::build_rpc_ops_filter;
pub use scope::{UnverifiedAuthPolicy, VerifiedPrincipal};
pub use service::RateLimitService;
pub use store::{DEFAULT_MAX_BUCKETS, InMemoryRateLimitStore, RateLimitStore};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_budget;
#[cfg(test)]
mod tests_budget_caps;
#[cfg(test)]
mod tests_budget_store;
#[cfg(test)]
mod tests_evasion;
#[cfg(test)]
mod tests_evasion_v4;
#[cfg(test)]
mod tests_key_fn;
#[cfg(test)]
mod tests_scope;
#[cfg(test)]
mod tests_scope_ipv6;
#[cfg(test)]
mod tests_store_error;
#[cfg(test)]
mod tests_store_timeout;
#[cfg(test)]
mod tests_support;
#[cfg(test)]
mod tests_typed_bodies;
