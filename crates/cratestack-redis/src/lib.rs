//! Redis-backed server-side infrastructure for cratestack.
//!
//! Today this is a single piece: [`RedisIdempotencyStore`], the Redis
//! implementation of [`cratestack_axum::idempotency::IdempotencyStore`]
//! used by [`cratestack_axum::idempotency::IdempotencyLayer`]. The
//! Postgres equivalent lives in `cratestack-sqlx`; this crate is its
//! sibling for deployments that prefer Redis as the durable store for
//! banking-grade idempotency guarantees.
//!
//! The client-side `ClientStateStore` Redis implementation (for the
//! generated Rust client's request journal) is a separate concern and
//! lives in `cratestack-client-store-redis`.

pub mod idempotency;

pub use idempotency::{RedisIdempotencyStore, RedisIdempotencyStoreConfig};
