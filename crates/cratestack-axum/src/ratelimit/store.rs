use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use cratestack_core::{BoundedOutcome, Charged, ConsumeRequest, CratestackError, bucket_ttl_secs};

use super::config::{RateLimitConfig, RateLimitDecision};

mod buckets;
mod scopes;

// Re-export from cratestack-core for internal use
pub use cratestack_core::RateLimitStore;

use buckets::Buckets;
use scopes::Scopes;

/// Default ceiling on live buckets.
///
/// The in-memory store documents itself as single-replica/development
/// scale, and 100k live token buckets is far past that — a deployment
/// legitimately tracking that many distinct callers in one process wants
/// the Redis store, whose keyspace is not this process's heap. The cap
/// exists as a backstop *under* the cardinality budget, not instead of it:
/// with the budget doing its job the map is O(peers × 128), and the cap is
/// what keeps "peers" from being the unbounded term when a botnet supplies
/// them.
pub const DEFAULT_MAX_BUCKETS: usize = 100_000;

#[derive(Debug, Default)]
struct State {
    buckets: Buckets,
    scopes: Scopes,
    /// The widest budget window seen so far, used as the scope-eviction
    /// horizon. Tracked rather than assumed because the sweep runs on the
    /// bucket schedule and has no budget in hand at that moment; using the
    /// bucket TTL instead would silently reset a longer-than-TTL budget
    /// window early, handing an attacker a fresh cardinality allowance.
    max_window: Duration,
}

/// In-memory `RateLimitStore`. Suitable for single-replica deployments and
/// development; banks running multi-replica clusters need a Redis-backed
/// implementation so the limit is enforced cluster-wide.
///
/// Bounded in two independent ways since cratestack#871: an amortised
/// sweep drops buckets idle for a full [`cratestack_core::bucket_ttl_secs`]
/// (the same horizon Redis's `EXPIRE` uses), and [`Self::with_max_buckets`]
/// caps live buckets outright, failing closed when a sweep frees nothing.
#[derive(Debug, Clone)]
pub struct InMemoryRateLimitStore {
    state: Arc<Mutex<State>>,
    max_buckets: Option<usize>,
}

impl Default for InMemoryRateLimitStore {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(State::default())),
            max_buckets: Some(DEFAULT_MAX_BUCKETS),
        }
    }
}

impl InMemoryRateLimitStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Hard ceiling on live buckets, defaulting to [`DEFAULT_MAX_BUCKETS`].
    ///
    /// At the ceiling, a request for a bucket that does not exist yet is
    /// refused with `CratestackError::Internal` — a *logical* failure, so
    /// it stays closed under every [`super::StoreErrorPolicy`]. Requests
    /// for buckets that already exist keep being served normally.
    pub fn with_max_buckets(mut self, max_buckets: usize) -> Self {
        self.max_buckets = Some(max_buckets);
        self
    }

    /// Remove the ceiling entirely, leaving only the TTL sweep. For
    /// deployments that would rather risk the heap than refuse a caller.
    pub fn without_max_buckets(mut self) -> Self {
        self.max_buckets = None;
        self
    }

    /// Test seam: consume against an injected clock.
    ///
    /// Eviction is a clock decision, and a test that sleeps through a real
    /// 60s TTL is a test nobody runs. Precedent:
    /// `cratestack_core::log_throttle::LogThrottle::check_at`.
    #[doc(hidden)]
    pub fn _consume_at(
        &self,
        request: ConsumeRequest<'_>,
        now: Instant,
    ) -> Result<BoundedOutcome, CratestackError> {
        let ttl = Duration::from_secs(bucket_ttl_secs(request.config));
        let mut state = self
            .state
            .lock()
            .map_err(|_| CratestackError::Internal("rate limit store poisoned".to_owned()))?;

        if let Some(budget) = request.budget
            && budget.window > state.max_window
        {
            state.max_window = budget.window;
        }
        let max_window = state.max_window;
        if state.buckets.maybe_sweep(now, ttl) {
            state.scopes.sweep(now, max_window);
        }

        let charged = match request.budget {
            None => Charged::Requested,
            Some(budget) if state.scopes.admit(budget, request.key, now) => Charged::Requested,
            Some(_) => Charged::Fallback,
        };
        let decision = state.buckets.consume(
            request.charged_key(charged),
            request.config,
            now,
            self.max_buckets,
            ttl,
        )?;
        Ok(BoundedOutcome::new(decision, charged))
    }

    /// Test seam: how many buckets are live right now. The number the
    /// cratestack#871 regression tests assert a bound on.
    #[doc(hidden)]
    pub fn _bucket_count(&self) -> usize {
        self.state
            .lock()
            .map(|state| state.buckets.len())
            .unwrap_or(0)
    }
}

#[async_trait]
impl RateLimitStore for InMemoryRateLimitStore {
    async fn consume(
        &self,
        key: &str,
        config: RateLimitConfig,
    ) -> Result<RateLimitDecision, CratestackError> {
        self.consume_bounded(ConsumeRequest::new(key, config, None))
            .await
            .map(|outcome| outcome.decision)
    }

    async fn consume_bounded(
        &self,
        request: ConsumeRequest<'_>,
    ) -> Result<BoundedOutcome, CratestackError> {
        self._consume_at(request, Instant::now())
    }
}
