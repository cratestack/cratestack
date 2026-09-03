use std::time::{Duration, Instant};

use super::{LogThrottle, ThrottleDecision};

const INTERVAL: Duration = Duration::from_secs(10);

#[test]
fn the_very_first_call_always_emits() {
    let throttle = LogThrottle::new(INTERVAL);
    assert_eq!(
        throttle.check_at(Instant::now()),
        ThrottleDecision::Emit {
            suppressed_since_last: 0
        },
        "an operator must not have to wait out an interval to learn something began failing"
    );
}

#[test]
fn calls_inside_the_interval_are_suppressed_and_counted() {
    let throttle = LogThrottle::new(INTERVAL);
    let start = Instant::now();
    let _ = throttle.check_at(start);

    for offset in 1..=4 {
        assert_eq!(
            throttle.check_at(start + Duration::from_secs(offset)),
            ThrottleDecision::Suppress
        );
    }

    assert_eq!(
        throttle.check_at(start + INTERVAL),
        ThrottleDecision::Emit {
            suppressed_since_last: 4
        },
        "the swallowed count is what keeps the throttled line honest about the blast radius"
    );
}

#[test]
fn the_suppressed_count_resets_after_each_emit() {
    let throttle = LogThrottle::new(INTERVAL);
    let start = Instant::now();
    let _ = throttle.check_at(start);
    let _ = throttle.check_at(start + Duration::from_secs(1));
    let _ = throttle.check_at(start + INTERVAL);

    assert_eq!(
        throttle.check_at(start + INTERVAL + INTERVAL),
        ThrottleDecision::Emit {
            suppressed_since_last: 0
        }
    );
}
