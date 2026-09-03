//! Pluggable storage traits for idempotency, rate limiting, and client state,
//! shared between transport and backend-runtime crates.

pub mod client_state;
pub mod idempotency;
pub mod ratelimit;

pub use client_state::{
    ClientStateStore, InMemoryStateStore, JsonFileStateStore, PersistedClientState,
    RequestJournalEntry,
};
pub use idempotency::IdempotencyStore;
pub use ratelimit::{
    BoundedOutcome, BucketBudget, Charged, ConsumeRequest, RateLimitConfig, RateLimitDecision,
    RateLimitStore, bucket_ttl_secs,
};
