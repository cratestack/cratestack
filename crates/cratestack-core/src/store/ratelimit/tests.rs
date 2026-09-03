use std::time::Duration;

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

/// The values below are the ones the Lua script computed in-place before
/// cratestack#871 moved the formula here. Asserting the exact numbers is
/// the point: this is a drift guard, not a smoke test.
#[test]
fn bucket_ttl_matches_the_lua_formula_it_replaced() {
    // ceil(100 / 1.0) + 60
    assert_eq!(bucket_ttl_secs(RateLimitConfig::new(100, 1.0)), 160);
    // ceil(3 / 0.001) + 60 = 3060
    assert_eq!(bucket_ttl_secs(RateLimitConfig::new(3, 0.001)), 3060);
}

#[test]
fn bucket_ttl_is_clamped_at_both_ends() {
    // Fast refill would give 61s from the +60 alone; the floor is 60.
    assert_eq!(bucket_ttl_secs(RateLimitConfig::new(1, 1000.0)), 61);
    assert_eq!(bucket_ttl_secs(RateLimitConfig::new(0, 1000.0)), 60);
    // Glacial refill saturates at 24h rather than growing without bound.
    assert_eq!(
        bucket_ttl_secs(RateLimitConfig::new(u32::MAX, 0.001)),
        86_400
    );
}

/// A never-refilling bucket has no "time to refill" to derive a TTL from,
/// so it takes the 24h ceiling rather than dividing by zero.
#[test]
fn bucket_ttl_handles_non_positive_refill() {
    assert_eq!(bucket_ttl_secs(RateLimitConfig::new(10, 0.0)), 86_400);
    assert_eq!(bucket_ttl_secs(RateLimitConfig::new(10, -1.0)), 86_400);
    assert_eq!(bucket_ttl_secs(RateLimitConfig::new(10, f64::NAN)), 86_400);
}

/// cratestack#871 review, blocker 2: the scope lifetime must never be
/// shorter than the buckets it admitted, or a fresh scope re-admits
/// `max_distinct` more while the previous generation is still alive.
#[test]
fn scope_ttl_is_never_shorter_than_the_bucket_ttl() {
    // The measured regression: window 1s, bucket TTL 5060s. A 1s scope
    // let 21 buckets accumulate over five windows for a cap of 4.
    let config = RateLimitConfig::new(5000, 1.0);
    assert_eq!(bucket_ttl_secs(config), 5060);
    assert_eq!(scope_ttl_secs(config, Duration::from_secs(1)), 5060);
}

#[test]
fn scope_ttl_honours_a_window_longer_than_the_bucket_ttl() {
    let config = RateLimitConfig::new(1, 1000.0); // bucket TTL 61s
    assert_eq!(scope_ttl_secs(config, Duration::from_secs(3600)), 3600);
}

/// should-fix 4: a `Duration::MAX` window used to reach Redis's `PEXPIRE`
/// as an out-of-range integer, failing `consume` with `Internal` — which
/// 500s every rate-limited route. A nonsensical budget must degrade, not
/// take the service down.
#[test]
fn scope_ttl_clamps_every_degenerate_window() {
    let config = RateLimitConfig::new(10, 1.0); // bucket TTL 70s
    assert_eq!(scope_ttl_secs(config, Duration::ZERO), 70);
    assert_eq!(scope_ttl_secs(config, Duration::from_millis(1)), 70);
    assert_eq!(scope_ttl_secs(config, Duration::MAX), MAX_TTL_SECS);
    assert_eq!(
        scope_ttl_secs(config, Duration::from_secs(u64::MAX / 2)),
        MAX_TTL_SECS,
    );
    // Even the widest bucket TTL stays inside the ceiling.
    let never_refills = RateLimitConfig::new(10, 0.0);
    assert!(scope_ttl_secs(never_refills, Duration::ZERO) <= MAX_TTL_SECS);
}

#[test]
fn charged_key_resolves_to_the_fallback_only_when_over_budget() {
    let budget = BucketBudget::new("peer:192.0.2.1", "ip:192.0.2.1", 8, Duration::from_secs(60));
    let request = ConsumeRequest::new("auth:abc", RateLimitConfig::new(1, 1.0), Some(&budget));

    assert_eq!(request.charged_key(Charged::Requested), "auth:abc");
    assert_eq!(request.charged_key(Charged::Fallback), "ip:192.0.2.1");
    assert_eq!(request.charged_key(Charged::Overflow), "ip:192.0.2.1");
    assert_eq!(request.charged_key(Charged::Unbounded), "auth:abc");
}

#[test]
fn charged_key_without_a_budget_is_always_the_requested_key() {
    let request = ConsumeRequest::new("ip:192.0.2.1", RateLimitConfig::new(1, 1.0), None);
    assert_eq!(request.charged_key(Charged::Fallback), "ip:192.0.2.1");
}

/// A store written against the pre-cratestack#871 trait must keep
/// compiling *and* keep behaving — the default `consume_bounded` delegates
/// and says so, rather than silently pretending a bound was applied.
#[tokio::test]
async fn default_consume_bounded_delegates_and_reports_unbounded() {
    struct LegacyStore;

    #[async_trait]
    impl RateLimitStore for LegacyStore {
        async fn consume(
            &self,
            _key: &str,
            _config: RateLimitConfig,
        ) -> Result<RateLimitDecision, CratestackError> {
            Ok(RateLimitDecision::Allowed { remaining: 3 })
        }
    }

    let budget = BucketBudget::new("peer:x", "ip:x", 1, Duration::from_secs(60));
    let outcome = LegacyStore
        .consume_bounded(ConsumeRequest::new(
            "auth:whatever",
            RateLimitConfig::new(5, 1.0),
            Some(&budget),
        ))
        .await
        .expect("legacy store consumes");

    assert_eq!(
        outcome.decision,
        RateLimitDecision::Allowed { remaining: 3 }
    );
    assert_eq!(outcome.charged, Charged::Unbounded);
}
