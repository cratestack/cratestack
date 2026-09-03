//! The deployment-tier knobs for cratestack#871's keyspace bound, and the
//! throttled `WARN`s the bound emits.

use std::time::Duration;

pub(super) mod warn;

/// Caps on how many distinct rate-limit buckets one scope may create,
/// applied by [`super::RateLimitLayer`]'s default key derivation.
///
/// # Where the defaults come from
///
/// `max_distinct_per_peer = 128` is sized so that no realistic *legitimate*
/// peer reaches it: a single NAT egress (or IPv6 /64) serving 128
/// simultaneously-active distinct credentials within one scope lifetime is
/// already an unusual deployment, and one that should be configuring this rather than
/// inheriting it. An attacker, by contrast, needs one bucket per request
/// to amplify — so 128 is three orders of magnitude below what the attack
/// needs while still above what real traffic uses.
///
/// `max_distinct_global = 8192` applies only when there is no verified
/// peer address at all (no `ConnectInfo`), where every caller shares one
/// scope. It is deliberately far larger, because collateral there hits
/// *unrelated* callers: the global scope degrades to a single loud
/// overflow bucket only when the deployment is both misconfigured and
/// under attack.
///
/// `window` (default 60s) is a **floor** on how long one admitted
/// credential holds its slot, not a fixed period that resets the scope.
/// The store raises it to at least the buckets' own TTL
/// (`cratestack_core::scope_ttl_secs`), because a record that expired
/// first bounded nothing — the next generation re-admitted `max_distinct`
/// more while the previous one was still alive, for a real steady state of
/// `max_distinct × ceil(bucket_ttl / window)` (cratestack#871 review,
/// blocker 2). Slots expire individually and are refreshed on use, so an
/// active credential keeps its slot while a rotated-away one releases it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitBucketBudget {
    pub max_distinct_per_peer: u32,
    pub max_distinct_global: u32,
    pub window: Duration,
}

impl RateLimitBucketBudget {
    pub const DEFAULT_MAX_DISTINCT_PER_PEER: u32 = 128;
    pub const DEFAULT_MAX_DISTINCT_GLOBAL: u32 = 8192;
    pub const DEFAULT_WINDOW: Duration = Duration::from_secs(60);

    pub fn new(max_distinct_per_peer: u32, max_distinct_global: u32, window: Duration) -> Self {
        Self {
            max_distinct_per_peer,
            max_distinct_global,
            window,
        }
    }

    pub fn max_distinct_per_peer(mut self, max: u32) -> Self {
        self.max_distinct_per_peer = max;
        self
    }

    pub fn max_distinct_global(mut self, max: u32) -> Self {
        self.max_distinct_global = max;
        self
    }

    pub fn window(mut self, window: Duration) -> Self {
        self.window = window;
        self
    }
}

impl Default for RateLimitBucketBudget {
    fn default() -> Self {
        Self {
            max_distinct_per_peer: Self::DEFAULT_MAX_DISTINCT_PER_PEER,
            max_distinct_global: Self::DEFAULT_MAX_DISTINCT_GLOBAL,
            window: Self::DEFAULT_WINDOW,
        }
    }
}
