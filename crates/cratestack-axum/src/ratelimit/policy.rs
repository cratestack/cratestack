//! What the rate-limit layer does when the *store* itself fails
//! (cratestack#846), and how long it is willing to wait to find out. The
//! policy type and the default budget live in `cratestack-exec`; what
//! stays here is this layer's synthetic timeout error and its warnings.

use std::time::Duration;

use cratestack_core::CratestackError;
use cratestack_core::log_throttle::LogThrottle;

/// Moved to L3 (`cratestack-exec`) by cratestack#1038 so MCP takes the
/// same setting as this layer; re-exported here, and from
/// `crate::ratelimit`, so `cratestack_axum::ratelimit::StoreErrorPolicy`
/// and `DEFAULT_STORE_TIMEOUT` keep naming the very same type and constant
/// (`tests/store_error_policy_compat.rs` pins that old call sites compile).
pub use cratestack_exec::{DEFAULT_STORE_TIMEOUT, StoreErrorPolicy};

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
