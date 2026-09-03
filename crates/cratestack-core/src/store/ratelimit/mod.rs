//! Rate limiting store trait and configuration types.

use std::time::Duration;

use async_trait::async_trait;

use crate::CratestackError;

mod budget;

pub use budget::{BoundedOutcome, BucketBudget, Charged, ConsumeRequest};

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

/// How long an idle bucket stays relevant: the time to refill a full
/// bucket plus a minute of slack, clamped to `[60s, 24h]`.
///
/// Lives in `cratestack-core` rather than in either store because BOTH
/// need it and they must not drift: `cratestack-redis` passes the result
/// to its Lua script as the `EXPIRE` argument (the script used to compute
/// the same arithmetic itself), and `cratestack-axum`'s in-memory store
/// uses it as the eviction horizon for its sweep (cratestack#871). A
/// divergence would mean the two backends bound their keyspaces
/// differently for identical configuration — exactly the class of silent
/// backend-specific behaviour the store trait exists to prevent.
///
/// `refill_per_second <= 0` means the bucket never refills, so there is no
/// "time to refill" to derive from; it gets the 24h clamp.
pub fn bucket_ttl_secs(config: RateLimitConfig) -> u64 {
    const MIN_TTL_SECS: f64 = 60.0;
    const MAX_BUCKET_TTL_SECS: f64 = 86_400.0;
    if config.refill_per_second <= 0.0 || config.refill_per_second.is_nan() {
        return MAX_BUCKET_TTL_SECS as u64;
    }
    let ttl = (f64::from(config.burst) / config.refill_per_second).ceil() + 60.0;
    // `ttl` is non-NaN here (the guard above excludes NaN and the numerator
    // is finite), so `clamp` cannot panic; an infinite quotient lands on
    // MAX rather than saturating the cast.
    ttl.clamp(MIN_TTL_SECS, MAX_BUCKET_TTL_SECS) as u64
}

/// Ceiling on any store-side TTL, in seconds: one year.
///
/// Exists because `Duration` reaches 584 billion years and Redis's
/// `PEXPIRE` takes an `i64` of milliseconds — a `Duration::MAX` window
/// reached the script as an out-of-range integer, which failed the whole
/// `consume` with `Internal` and therefore 500'd every rate-limited route
/// (cratestack#871 review, should-fix 4). Clamping keeps a nonsensical
/// configuration a *degenerate* budget rather than an outage.
pub const MAX_TTL_SECS: u64 = 365 * 24 * 60 * 60;

/// How long a scope's admission record must live: at least as long as the
/// buckets it admitted, and at least the caller's requested floor.
///
/// This is the whole fix for cratestack#871's second blocker. A scope that
/// expires before its buckets do bounds nothing — the next generation
/// admits `max_distinct` more while the previous ones are still alive, so
/// the steady state was `max_distinct × ceil(bucket_ttl / window)` rather
/// than `max_distinct`. Shared between both backends for the same reason
/// [`bucket_ttl_secs`] is: a divergence here is a silently different bound
/// per backend.
pub fn scope_ttl_secs(config: RateLimitConfig, window: Duration) -> u64 {
    // Clamped at both ends. `Duration::MAX` is ~584 billion years, which
    // reached Redis's `PEXPIRE` as an out-of-range integer and failed the
    // whole `consume` with `Internal` — 500ing every rate-limited route.
    // And a zero-length lifetime would expire the record before the
    // request that created it could use it, which Redis rejects outright.
    window
        .as_secs()
        .max(bucket_ttl_secs(config))
        .clamp(1, MAX_TTL_SECS)
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
    ) -> Result<RateLimitDecision, CratestackError>;

    /// Consume one token, honouring a [`BucketBudget`] on how many
    /// *distinct* buckets the request's scope may create (cratestack#871).
    ///
    /// The default implementation ignores the budget and reports
    /// [`Charged::Unbounded`], so a third-party store written against the
    /// pre-#871 trait keeps compiling and behaving exactly as before — at
    /// the cost of leaving the keyspace unbounded for its deployment. The
    /// middleware logs that (throttled) rather than failing, because the
    /// alternative is breaking every out-of-tree store on upgrade.
    ///
    /// Implementations MUST keep the "does this scope already know this
    /// bucket / may it learn a new one / charge the fallback" decision
    /// atomic with the token consumption. Doing it as two round-trips
    /// re-opens the race the budget exists to close: N concurrent requests
    /// each observing `SCARD < max` all create a bucket.
    async fn consume_bounded(
        &self,
        request: ConsumeRequest<'_>,
    ) -> Result<BoundedOutcome, CratestackError> {
        let decision = self.consume(request.key, request.config).await?;
        Ok(BoundedOutcome {
            decision,
            charged: Charged::Unbounded,
        })
    }
}

#[cfg(test)]
mod tests;
