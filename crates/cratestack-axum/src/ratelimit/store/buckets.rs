//! The token-bucket map behind [`super::InMemoryRateLimitStore`], plus the
//! eviction that stops it growing forever (cratestack#871).
//!
//! Before #871 this map only ever grew: every distinct key created an
//! entry and nothing removed it, so a caller rotating an unverified
//! `Authorization` header leaked one entry per request for the lifetime of
//! the process. The Redis backend never had this problem — it sets an
//! `EXPIRE` on every write — so the fix here is to give the in-memory map
//! the same horizon, from the same shared formula
//! ([`cratestack_core::bucket_ttl_secs`]).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use cratestack_core::{CratestackError, RateLimitConfig, RateLimitDecision};

/// How often the sweep is allowed to run, at most.
///
/// Sweeping on every write would make each request O(map), which turns the
/// limiter itself into the bottleneck it is meant to protect against.
/// Amortising to once per minute costs, at worst, one extra TTL of
/// retention (entries can survive up to `ttl + SWEEP_INTERVAL`) — a
/// constant factor on a bound whose point is to be finite.
const SWEEP_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Debug)]
struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

#[derive(Debug, Default)]
pub(super) struct Buckets {
    map: HashMap<String, Bucket>,
    /// `None` until the first write. The first write does not sweep (an
    /// empty map has nothing to free) but does start the clock, so the
    /// first real sweep happens one interval into the process's life
    /// rather than on request one.
    last_sweep: Option<Instant>,
}

impl Buckets {
    pub(super) fn len(&self) -> usize {
        self.map.len()
    }

    /// Drop every bucket that has been idle for at least one full TTL.
    ///
    /// "Idle for a TTL" is the same condition Redis's `EXPIRE` encodes:
    /// such a bucket has necessarily refilled to `burst`, so recreating it
    /// on the next request yields a byte-identical state. Eviction here is
    /// therefore not an approximation — it cannot grant or deny a token
    /// that retaining the entry would have decided differently.
    pub(super) fn sweep(&mut self, now: Instant, ttl: Duration) {
        self.map
            .retain(|_, bucket| now.saturating_duration_since(bucket.last_refill) < ttl);
        self.last_sweep = Some(now);
    }

    /// Sweep at most once per [`SWEEP_INTERVAL`]; reports whether it ran,
    /// so the caller can sweep the scope index on the same schedule
    /// instead of keeping a second timer for it.
    pub(super) fn maybe_sweep(&mut self, now: Instant, ttl: Duration) -> bool {
        match self.last_sweep {
            None => {
                self.last_sweep = Some(now);
                false
            }
            Some(last) if now.saturating_duration_since(last) >= SWEEP_INTERVAL => {
                self.sweep(now, ttl);
                true
            }
            Some(_) => false,
        }
    }

    /// Consume one token from `key`, creating the bucket if the cap allows.
    ///
    /// The cap is checked only on *creation*: an existing bucket is always
    /// servable, so a deployment at its cap keeps throttling correctly for
    /// everyone it already knows about and refuses only the marginal new
    /// identity.
    pub(super) fn consume(
        &mut self,
        key: &str,
        config: RateLimitConfig,
        now: Instant,
        max_buckets: Option<usize>,
        ttl: Duration,
    ) -> Result<RateLimitDecision, CratestackError> {
        if !self.map.contains_key(key) {
            self.admit(key, now, max_buckets, ttl)?;
            self.map.insert(
                key.to_owned(),
                Bucket {
                    tokens: f64::from(config.burst),
                    last_refill: now,
                },
            );
        }
        let bucket = self
            .map
            .get_mut(key)
            .expect("bucket was just inserted or already present");
        let elapsed = now
            .saturating_duration_since(bucket.last_refill)
            .as_secs_f64();
        bucket.tokens =
            (bucket.tokens + elapsed * config.refill_per_second).min(config.burst.into());
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

    /// Fail CLOSED at the cap, deliberately.
    ///
    /// `Internal` — not `Unavailable` — because this is a logical failure:
    /// the store was reached and refused, it does not self-heal on its
    /// own, and it is reachable by a caller. `StoreErrorPolicy::Allow`
    /// serves through transport-class failures only, so classifying it
    /// this way is what makes the cap unbypassable; returning
    /// `Unavailable` here would hand an attacker "fill the map, then walk
    /// through unthrottled", which is exactly cratestack#846's bypass
    /// wearing a different hat.
    fn admit(
        &mut self,
        key: &str,
        now: Instant,
        max_buckets: Option<usize>,
        ttl: Duration,
    ) -> Result<(), CratestackError> {
        let Some(max) = max_buckets else {
            return Ok(());
        };
        if self.map.len() < max {
            return Ok(());
        }
        // One forced sweep before refusing: at the cap, an out-of-schedule
        // O(map) pass is cheaper than a refused request, and the common
        // case for hitting the cap at all is a burst that has since aged
        // out.
        self.sweep(now, ttl);
        if self.map.len() < max {
            return Ok(());
        }
        Err(CratestackError::Internal(format!(
            "rate limit store is at its {max}-bucket cap and a sweep freed nothing, so no bucket \
             could be created for a new caller identity (cratestack#871). Raise \
             InMemoryRateLimitStore::with_max_buckets, or move to a Redis-backed store: key={}",
            redacted(key)
        )))
    }
}

/// Bucket keys are hashes of caller-supplied material (`auth:<sha256>`) or
/// peer addresses. Neither is a secret, but neither belongs in an error
/// body either — `Internal`'s payload is operator-facing, so keep it to a
/// prefix that identifies the *shape* of the key that could not be made.
fn redacted(key: &str) -> &str {
    let end = key.find(':').map_or(0, |i| i + 1);
    &key[..end]
}
