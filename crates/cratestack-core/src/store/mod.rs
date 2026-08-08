//! Pluggable storage traits for idempotency and rate limiting, shared
//! between transport and backend-runtime crates.

pub mod idempotency;
pub mod ratelimit;

pub use idempotency::IdempotencyStore;
pub use ratelimit::{RateLimitConfig, RateLimitDecision, RateLimitStore};
