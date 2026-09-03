//! The four `WARN`s the keyspace bound can emit, each on its own throttle.
//!
//! All four fire on conditions a caller can drive at whatever rate they
//! choose (an amplification attempt IS the condition), so an unthrottled
//! line here would turn cratestack#871's defence into cratestack#871's log
//! amplifier. Same reasoning, and the same [`LogThrottle`], as the
//! store-error `WARN`s in `super::super::policy`.
//!
//! Owned per-layer rather than in `static`s, for that module's two stated
//! reasons: a process-global log budget makes any test asserting on these
//! lines order-dependent, and two routers with independent limiters have
//! no reason to silence each other.

use std::time::Duration;

use cratestack_core::log_throttle::{LogThrottle, ThrottleDecision};

/// A boot-time wiring defect, not a per-request condition — so the budget
/// is long. Matches the once-per-process treatment the same
/// `into_make_service_with_connect_info` mistake already gets in
/// `headers::enrich` and `super::super::key_fn`; a throttle rather than a
/// `Once` only so that an operator who fixes nothing still sees it again
/// eventually, and so tests are not order-dependent.
const WIRING_INTERVAL: Duration = Duration::from_secs(3600);

/// An in-progress amplification attempt. Frequent enough that an operator
/// should see movement during an incident, rare enough not to flood.
const ATTACK_INTERVAL: Duration = Duration::from_secs(60);

/// Each method returns whether it actually emitted, which is what the
/// tests assert on. Capturing `tracing` events from the async layer path
/// was tried and abandoned as scheduling-dependent (see
/// `super::super::tests_store_error`'s module docs); a return value is
/// deterministic and needs no subscriber.
#[derive(Debug)]
pub(crate) struct BudgetWarnings {
    missing_peer: LogThrottle,
    unbounded_store: LogThrottle,
    fallback: LogThrottle,
    overflow: LogThrottle,
}

impl Default for BudgetWarnings {
    fn default() -> Self {
        Self {
            missing_peer: LogThrottle::new(WIRING_INTERVAL),
            unbounded_store: LogThrottle::new(WIRING_INTERVAL),
            fallback: LogThrottle::new(ATTACK_INTERVAL),
            overflow: LogThrottle::new(ATTACK_INTERVAL),
        }
    }
}

impl BudgetWarnings {
    /// An `Authorization` header arrived with no verified peer address, so
    /// the bucket it mints is counted against the process-global scope
    /// instead of the caller's own. Not a refusal — that would break every
    /// deployment that authenticates but does not wire `ConnectInfo` — but
    /// it does mean unrelated callers share one budget.
    pub(crate) fn missing_peer(&self) -> bool {
        let ThrottleDecision::Emit {
            suppressed_since_last,
        } = self.missing_peer.check()
        else {
            return false;
        };
        tracing::warn!(
            target: "cratestack",
            cratestack_operation = "rate_limit",
            suppressed_since_last,
            "RateLimitLayer saw an Authorization header but no ConnectInfo<SocketAddr> peer, \
             so the bucket it derives is counted against a single process-global cardinality \
             budget shared with every other such caller (cratestack#871). Serve through \
             into_make_service_with_connect_info::<SocketAddr>() to get a per-peer budget, or \
             supply RateLimitLayer::with_key_fn(...) explicitly.",
        );
        true
    }

    /// The configured store does not implement `consume_bounded`, so the
    /// budget was computed and then ignored. Says so rather than letting a
    /// deployment believe it is bounded when it is not.
    pub(crate) fn unbounded_store(&self) -> bool {
        let ThrottleDecision::Emit {
            suppressed_since_last,
        } = self.unbounded_store.check()
        else {
            return false;
        };
        tracing::warn!(
            target: "cratestack",
            cratestack_operation = "rate_limit",
            suppressed_since_last,
            "the configured RateLimitStore does not implement consume_bounded, so the \
             rate-limit bucket keyspace is NOT bounded for this deployment \
             (cratestack#871): a caller rotating an unverified Authorization header can \
             still mint one bucket per request. Implement RateLimitStore::consume_bounded \
             in the store, or use InMemoryRateLimitStore / RedisRateLimitStore.",
        );
        true
    }

    /// A peer hit its per-peer cap: it is now sharing its own `ip:` bucket
    /// for every further distinct credential. Legitimate traffic does not
    /// reach this, so it reads as an attack indicator.
    pub(crate) fn fallback(&self, scope_key: &str) -> bool {
        let ThrottleDecision::Emit {
            suppressed_since_last,
        } = self.fallback.check()
        else {
            return false;
        };
        tracing::warn!(
            target: "cratestack",
            cratestack_operation = "rate_limit",
            rate_limit_scope = %scope_key,
            suppressed_since_last,
            "rate-limit bucket cardinality budget exhausted for this scope; further distinct \
             credentials from it are charged to its fallback bucket (cratestack#871). This is \
             what an Authorization-rotation amplification attempt looks like.",
        );
        true
    }

    /// The *global* scope hit its cap. Strictly worse than
    /// [`Self::fallback`]: unrelated callers are now collapsed onto one
    /// overflow bucket, which is the only case in this design where one
    /// caller can consume another's budget.
    pub(crate) fn overflow(&self) -> bool {
        let ThrottleDecision::Emit {
            suppressed_since_last,
        } = self.overflow.check()
        else {
            return false;
        };
        tracing::warn!(
            target: "cratestack",
            cratestack_operation = "rate_limit",
            suppressed_since_last,
            "the PROCESS-GLOBAL rate-limit bucket cardinality budget is exhausted \
             (cratestack#871). Every further unverified Authorization value now shares one \
             overflow bucket, so unrelated callers can throttle each other. This deployment \
             is both missing ConnectInfo<SocketAddr> wiring and under an amplification \
             attempt.",
        );
        true
    }
}
