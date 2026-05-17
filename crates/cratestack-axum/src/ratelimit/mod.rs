//! Per-principal rate limiting.
//!
//! Token-bucket algorithm with a pluggable store. The default in-memory
//! implementation is appropriate for single-instance deployments; banks
//! running multiple replicas bring a Redis-backed implementation through
//! the [`RateLimitStore`] trait so all replicas share the same view of
//! consumption.
//!
//! The middleware computes a key per request (the default is the
//! authorization-header fingerprint, the same shape the idempotency layer
//! uses) and refuses with `429` plus a `Retry-After` header when the bucket
//! is empty. Banks running tenant-scoped budgeting can swap the key
//! function for tenant-id.

mod config;
mod layer;
mod store;

pub use config::{_bucket_capacity_for, RateLimitConfig, RateLimitDecision};
pub use layer::{RateLimitLayer, RateLimitService};
pub use store::{InMemoryRateLimitStore, RateLimitStore};

#[cfg(test)]
mod tests;
