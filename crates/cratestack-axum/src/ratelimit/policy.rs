//! What the rate-limit layer does when the *store* itself fails
//! (cratestack#846), and how long it is willing to wait to find out.

use std::time::Duration;

use cratestack_core::CratestackError;
use cratestack_core::log_throttle::LogThrottle;

/// How [`super::RateLimitLayer`] treats a failure of the backing
/// [`super::RateLimitStore`], as distinct from a caller who is genuinely
/// over budget.
///
/// # The distinction that matters is transport vs logical, not open vs closed
///
/// The first cut of this knob was "on any store error, allow". A security
/// review falsified the premise it rested on — that a store failure is
/// never caller-controlled — with a measured attack:
/// [`super::key_fn::default_key_fn`] hashes an **unvalidated**
/// `Authorization` header (this layer runs before authentication), so an
/// unauthenticated caller mints one Redis key per request just by
/// rotating that header. Drive that until the instance reaches
/// `maxmemory` and every subsequent `HSET` fails with `OOM` — at which
/// point a blanket fail-open serves *every* request unthrottled,
/// including from buckets that were already exhausted. The bypass is
/// reachable by anyone.
///
/// So the axis is not "open vs closed". It is:
///
/// - A **transport** failure — the socket broke, the server is
///   unreachable — is not caller-controlled and self-heals once the
///   connection is replaced. Refusing here converts a limiter hiccup into
///   a simultaneous outage of every rate-limited route, for a condition
///   nobody in the request path can fix. This is what
///   [`StoreErrorPolicy::Allow`] serves through.
/// - A **logical** failure — the store was reached and said no (`OOM`, a
///   permission error, a poisoned mutex, a malformed reply) — may be
///   caller-induced, does not self-heal, and is exactly the shape an
///   attacker steers toward. It stays closed under **every** policy.
///
/// Concretely: `Allow` matches [`CratestackError::Unavailable`] and nothing
/// else. Backends signal transport-class failures with that variant
/// (`cratestack-redis`'s `ratelimit::util::is_transport_class`); anything
/// else they return is refused even under `Allow`.
///
/// Key derivation remains fail-closed under both policies (cratestack#416)
/// for the same reason the OOM case is: its inputs are caller-controlled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum StoreErrorPolicy {
    /// Serve the request unthrottled when — and only when — the store
    /// failure is transport-class. Every other store failure is refused
    /// exactly as under [`StoreErrorPolicy::Deny`].
    #[default]
    Allow,
    /// Refuse on any store failure, transport-class included, with the
    /// store's own error status and the normal typed error envelope.
    /// For deployments where the limiter is a security control (a
    /// paywall, a brute-force guard) rather than a capacity control.
    Deny,
}

impl StoreErrorPolicy {
    /// Whether this policy serves `error` through to the inner service.
    ///
    /// Deliberately a match on the error *variant* rather than on
    /// `status_code()`: 503 is also reachable from a hand-written store
    /// that means something else by it, and a helper whose job is to gate
    /// a security-relevant bypass should be readable without a detour
    /// through the HTTP mapping table.
    pub(super) fn permits(self, error: &CratestackError) -> bool {
        match self {
            Self::Deny => false,
            Self::Allow => matches!(error, CratestackError::Unavailable(_)),
        }
    }
}

/// Default ceiling on one `store.consume` call — first attempt *and* any
/// backend-internal retry, as a single budget.
///
/// The security review measured the alternative: `redis`'s
/// `ConnectionManager` defaults both its connection and response timeouts
/// to `None`, so during a real outage each attempt awaited an unbounded
/// reconnect cycle — 9.46s, doubled to 18.92s by the retry. "Degrade to
/// unlimited" silently meant "hang for nineteen seconds, then allow",
/// which is worse for the caller than the refusal it replaced and is
/// itself a denial-of-service lever.
///
/// 500ms is chosen to be far above a healthy Redis round-trip (sub-
/// millisecond on a local network, single-digit milliseconds across an
/// availability zone) and far below anything a human would call a hang.
/// Tune with [`super::RateLimitLayer::with_store_timeout`].
pub const DEFAULT_STORE_TIMEOUT: Duration = Duration::from_millis(500);

/// Message carried by the synthetic error a budget elapse produces. A
/// timeout IS a transport-class failure — the store did not answer — so
/// it is reported as [`CratestackError::Unavailable`] and is therefore
/// servable under `Allow`, unlike an `OOM`.
pub(super) fn store_timeout_error() -> CratestackError {
    CratestackError::Unavailable("rate limit store timed out".to_owned())
}

/// The two throttled `WARN`s the store-error path emits.
///
/// Owned per-layer rather than kept in `static`s. Two reasons, in order
/// of importance: a process-global log budget is shared mutable state
/// that makes any test asserting on these lines order-dependent (the
/// first call in a process always emits, so whichever test runs first
/// wins); and a process hosting two routers with independent limiters
/// has no reason to make one limiter's outage silence the other's.
#[derive(Debug)]
pub(super) struct StoreErrorWarnings {
    /// The per-request "store error" line. Throttled because the
    /// condition is attacker-drivable: during an outage it fires once per
    /// request, at whatever rate the caller chooses, so leaving it
    /// unthrottled turns a store failure into a log-volume amplifier on
    /// top of everything else. The suppressed count travels in the
    /// message so the throttle never understates the blast radius.
    pub(super) store_error: LogThrottle,
    /// Separate budget, not a second use of the one above: this line says
    /// "we are now serving unthrottled", which an operator must keep
    /// seeing at a predictable cadence during a long outage. The first
    /// cut used a `Once`, which under-reported badly — a limiter that
    /// stops limiting for an hour deserves more than one line an hour
    /// ago.
    pub(super) fail_open: LogThrottle,
}

impl Default for StoreErrorWarnings {
    fn default() -> Self {
        Self {
            store_error: LogThrottle::new(Duration::from_secs(10)),
            fail_open: LogThrottle::new(Duration::from_secs(60)),
        }
    }
}
