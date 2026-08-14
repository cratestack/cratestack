use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

use async_trait::async_trait;
use cratestack_core::CratestackError;

use super::config::{RateLimitConfig, RateLimitDecision};

// Re-export from cratestack-core for internal use
pub use cratestack_core::RateLimitStore;

#[derive(Debug)]
struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

/// In-memory `RateLimitStore`. Suitable for single-replica deployments and
/// development; banks running multi-replica clusters need a Redis-backed
/// implementation so the limit is enforced cluster-wide.
#[derive(Debug, Clone, Default)]
pub struct InMemoryRateLimitStore {
    buckets: Arc<Mutex<HashMap<String, Bucket>>>,
}

impl InMemoryRateLimitStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl RateLimitStore for InMemoryRateLimitStore {
    async fn consume(
        &self,
        key: &str,
        config: RateLimitConfig,
    ) -> Result<RateLimitDecision, CratestackError> {
        let mut buckets = self
            .buckets
            .lock()
            .map_err(|_| CratestackError::Internal("rate limit store poisoned".to_owned()))?;
        let now = Instant::now();
        let bucket = buckets.entry(key.to_owned()).or_insert(Bucket {
            tokens: config.burst as f64,
            last_refill: now,
        });
        let elapsed = now
            .saturating_duration_since(bucket.last_refill)
            .as_secs_f64();
        bucket.tokens =
            (bucket.tokens + elapsed * config.refill_per_second).min(config.burst as f64);
        bucket.last_refill = now;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(RateLimitDecision::Allowed {
                remaining: bucket.tokens.floor() as u32,
            })
        } else {
            let need = 1.0 - bucket.tokens;
            let secs = (need / config.refill_per_second).ceil() as u32;
            Ok(RateLimitDecision::Throttled {
                retry_after_secs: secs.max(1),
            })
        }
    }
}
