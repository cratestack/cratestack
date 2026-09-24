//! Rate-limit stores that do not answer normally: one that fails with a
//! chosen error, one that never answers at all. They exist to pin what a
//! store outage does to an MCP call, which a counting store cannot show.

use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use cratestack_core::{CratestackError, RateLimitConfig, RateLimitDecision, RateLimitStore};

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
