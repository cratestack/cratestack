//! Stores that do not answer normally: rate-limit stores that fail with a
//! chosen error or never answer at all, and an idempotency store that is
//! unreachable. They exist to pin what a store outage does to an MCP call,
//! which a counting store cannot show.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

use async_trait::async_trait;
use cratestack_core::idempotency_record::ReservationOutcome;
use cratestack_core::{
    CratestackError, IdempotencyStore, RateLimitConfig, RateLimitDecision, RateLimitStore,
};

/// Fails every `consume` with the error `make` builds.
pub struct FailingLimiter {
    make: fn() -> CratestackError,
}

impl FailingLimiter {
    pub fn new(make: fn() -> CratestackError) -> Self {
        Self { make }
    }
}

#[async_trait]
impl RateLimitStore for FailingLimiter {
    async fn consume(
        &self,
        _key: &str,
        _config: RateLimitConfig,
    ) -> Result<RateLimitDecision, CratestackError> {
        Err((self.make)())
    }
}

/// Never answers: the shape of a Redis outage behind a connection manager
/// with no timeouts, which is what `cratestack-axum`'s store timeout was
/// measured against (`ratelimit/policy.rs`, `DEFAULT_STORE_TIMEOUT`).
#[derive(Default)]
pub struct HungLimiter {
    pub calls: AtomicUsize,
}

#[async_trait]
impl RateLimitStore for HungLimiter {
    async fn consume(
        &self,
        _key: &str,
        _config: RateLimitConfig,
    ) -> Result<RateLimitDecision, CratestackError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        std::future::pending().await
    }
}

/// An idempotency store whose every call fails transport-class
/// (`Unavailable`), the one error `StoreErrorPolicy::Allow` serves through
/// on the *rate-limit* path. Counts reservations so a test can tell "never
/// asked" from "asked and refused".
#[derive(Default)]
pub struct UnreachableIdempotency {
    pub reservations: AtomicUsize,
}

fn unreachable() -> CratestackError {
    CratestackError::Unavailable("idempotency store down".to_owned())
}

#[async_trait]
impl IdempotencyStore for UnreachableIdempotency {
    async fn reserve_or_fetch(
        &self,
        _principal: &str,
        _key: &str,
        _request_hash: [u8; 32],
        _expires_at: SystemTime,
    ) -> Result<ReservationOutcome, CratestackError> {
        self.reservations.fetch_add(1, Ordering::SeqCst);
        Err(unreachable())
    }

    async fn complete(
        &self,
        _principal: &str,
        _key: &str,
        _token: uuid::Uuid,
        _status: u16,
        _headers: &[u8],
        _body: &[u8],
    ) -> Result<(), CratestackError> {
        Err(unreachable())
    }

    async fn release(
        &self,
        _principal: &str,
        _key: &str,
        _token: uuid::Uuid,
    ) -> Result<(), CratestackError> {
        Err(unreachable())
    }
}
