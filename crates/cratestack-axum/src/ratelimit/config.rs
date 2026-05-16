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
