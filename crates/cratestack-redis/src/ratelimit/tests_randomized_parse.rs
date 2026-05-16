#![cfg(test)]

// Randomized property tests for outcome parsing and decimal helpers.

use cratestack_axum::ratelimit::RateLimitDecision;
use cratestack_core::CoolError;
use redis::Value as RedisValue;

use super::parse::{next_u32_decimal, parse_consume_outcome};
use super::tests_fixtures::{bulk, test_seed, XorShift64};

#[test]
fn randomized_parse_allowed_roundtrips_remaining() {
    let seed = test_seed();
    let mut rng = XorShift64::new(seed);
    for iteration in 0..200 {
        let remaining = rng.next_u32();
        let value = RedisValue::Array(vec![bulk("allowed"), bulk(&remaining.to_string())]);
        let outcome = parse_consume_outcome(value).unwrap_or_else(|err| {
            panic!("seed={seed:#x} iter={iteration} remaining={remaining}: {err:?}")
        });
        assert_eq!(
            outcome,
            RateLimitDecision::Allowed { remaining },
            "seed={seed:#x} iter={iteration}",
        );
    }
}

#[test]
fn randomized_parse_throttled_roundtrips_retry_after() {
    let seed = test_seed();
    let mut rng = XorShift64::new(seed);
    for iteration in 0..200 {
        // retry_after must be >= 1 in our wire format; clamp.
        let retry = rng.next_range(1, u32::MAX);
        let value = RedisValue::Array(vec![bulk("throttled"), bulk(&retry.to_string())]);
        let outcome = parse_consume_outcome(value).unwrap_or_else(|err| {
            panic!("seed={seed:#x} iter={iteration} retry={retry}: {err:?}")
        });
        assert_eq!(
            outcome,
            RateLimitDecision::Throttled { retry_after_secs: retry },
            "seed={seed:#x} iter={iteration}",
        );
    }
}

#[test]
fn randomized_parse_rejects_out_of_u32_range_remaining() {
    // Values above u32::MAX or below 0 must error rather than wrap.
    let seed = test_seed();
    let mut rng = XorShift64::new(seed);
    for iteration in 0..50 {
        let oversized: i64 = (u32::MAX as i64) + 1 + (rng.next_u64() as i64).abs() % 1_000_000;
        let value =
            RedisValue::Array(vec![bulk("allowed"), bulk(&oversized.to_string())]);
        let err = parse_consume_outcome(value).expect_err(&format!(
            "seed={seed:#x} iter={iteration} oversized={oversized}: must reject",
        ));
        assert!(matches!(err, CoolError::Internal(_)));
    }
}

#[test]
fn randomized_next_u32_decimal_round_trips_string_form() {
    let seed = test_seed();
    let mut rng = XorShift64::new(seed);
    for iteration in 0..200 {
        let n = rng.next_u32();
        let mut iter = vec![bulk(&n.to_string())].into_iter();
        let parsed = next_u32_decimal(&mut iter, "x").unwrap_or_else(|err| {
            panic!("seed={seed:#x} iter={iteration} n={n}: {err:?}")
        });
        assert_eq!(parsed, n);
    }
}

#[test]
fn randomized_next_u32_decimal_rejects_garbage_bytes() {
    let seed = test_seed();
    let mut rng = XorShift64::new(seed);
    for iteration in 0..50 {
        // Random bytes that almost certainly aren't decimal numbers.
        let bytes = rng.next_bytes(8);
        let payload = RedisValue::BulkString(bytes.clone());
        let mut iter = vec![payload].into_iter();
        let result = next_u32_decimal(&mut iter, "x");
        // Either it parses (extremely unlikely for random bytes) or
        // it errors cleanly — must never panic.
        if let Ok(n) = result {
            // Sanity: if it parsed, the string form must round-trip.
            let s = String::from_utf8(bytes).unwrap_or_default();
            assert_eq!(
                s.trim().parse::<u32>().ok(),
                Some(n),
                "seed={seed:#x} iter={iteration}: accepted bytes must be a valid u32 string",
            );
        }
    }
}
