//! Redis-backed server-side infrastructure for cratestack.
//!
//! Two stores live here:
//!
//! - [`RedisIdempotencyStore`] — Redis implementation of
//!   [`cratestack_axum::idempotency::IdempotencyStore`], used by
//!   [`cratestack_axum::idempotency::IdempotencyLayer`].
//! - [`RedisRateLimitStore`] — Redis implementation of
//!   [`cratestack_axum::ratelimit::RateLimitStore`], used by
//!   [`cratestack_axum::ratelimit::RateLimitLayer`] so token-bucket
//!   state is shared across replicas.
//!
//! The Postgres equivalent of the idempotency store lives in
//! `cratestack-sqlx`; this crate is its sibling for deployments that
//! prefer Redis as the durable store for banking-grade guarantees.
//!
//! The client-side `ClientStateStore` Redis implementation (for the
//! generated Rust client's request journal) is a separate concern and
//! lives in `cratestack-client-store-redis`.

pub mod idempotency;
pub mod ratelimit;

pub use idempotency::{RedisIdempotencyStore, RedisIdempotencyStoreConfig};
pub use ratelimit::{RedisRateLimitStore, RedisRateLimitStoreConfig};
