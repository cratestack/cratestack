//! A rate-limit store that fails with a chosen error. It exists to pin
//! what a store outage does to an MCP call, which a counting store cannot
//! show.

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
