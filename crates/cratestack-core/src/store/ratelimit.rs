//! Rate limiting store trait and configuration types.

use async_trait::async_trait;

use crate::CoolError;

/// Configuration for a single bucket: capacity (max burst) and refill rate
/// in tokens per second. Banks running high-frequency back-office traffic
/// pick large bursts; consumer-facing channels use small bursts to dampen
/// abuse.
#[derive(Debug, Clone, Copy)]
pub struct RateLimitConfig {
    pub burst: u32,
    pub refill_per_second: f64,
}

impl RateLimitConfig {
    pub fn new(burst: u32, refill_per_second: f64) -> Self {
        Self {
            burst,
            refill_per_second,
        }
    }
}

/// Result of attempting to consume a token. `Allowed` carries the number
/// of tokens left after consumption; `Throttled` carries seconds the
/// caller should wait before retrying.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RateLimitDecision {
    Allowed { remaining: u32 },
    Throttled { retry_after_secs: u32 },
}

/// Sleep helper for tests — exposes the bucket's wall-clock refill model so
/// the integration tests can exercise both the burst and the throttle path
/// without depending on real time.
#[doc(hidden)]
pub fn _bucket_capacity_for(config: RateLimitConfig) -> u32 {
    config.burst
}

/// Pluggable storage for token-bucket state. Implementations must be safe
/// to share across tasks (use a Mutex internally, or rely on the backing
/// store's atomicity).
#[async_trait]
pub trait RateLimitStore: Send + Sync + 'static {
    /// Atomically consume one token for `key`. Returns the decision based
    /// on the bucket state after the consumption attempt.
    async fn consume(
        &self,
        key: &str,
        config: RateLimitConfig,
    ) -> Result<RateLimitDecision, CoolError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limit_config_new_creates_correct_values() {
        let config = RateLimitConfig::new(100, 10.5);
        assert_eq!(config.burst, 100);
        assert_eq!(config.refill_per_second, 10.5);
    }

    #[test]
    fn rate_limit_decision_allowed_equality() {
        let d1 = RateLimitDecision::Allowed { remaining: 42 };
        let d2 = RateLimitDecision::Allowed { remaining: 42 };
        assert_eq!(d1, d2);
    }

    #[test]
    fn rate_limit_decision_throttled_equality() {
        let d1 = RateLimitDecision::Throttled {
            retry_after_secs: 5,
        };
        let d2 = RateLimitDecision::Throttled {
            retry_after_secs: 5,
        };
        assert_eq!(d1, d2);
    }

    #[test]
    fn bucket_capacity_for_returns_burst() {
        let config = RateLimitConfig::new(42, 1.0);
        assert_eq!(_bucket_capacity_for(config), 42);
    }
}
