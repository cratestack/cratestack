#![cfg(test)]

use cratestack_core::{Charged, CratestackError, RateLimitDecision};
use redis::Value as RedisValue;

use super::parse::parse_bounded_outcome;
use super::tests_fixtures::bulk;

#[test]
fn parse_allowed_extracts_remaining() {
    let value = RedisValue::Array(vec![bulk("allowed"), bulk("7"), bulk("requested")]);
    let outcome = parse_bounded_outcome(value).expect("parse").decision;
    assert_eq!(outcome, RateLimitDecision::Allowed { remaining: 7 });
}

#[test]
fn parse_throttled_extracts_retry_after() {
    let value = RedisValue::Array(vec![bulk("throttled"), bulk("3"), bulk("requested")]);
    let outcome = parse_bounded_outcome(value).expect("parse").decision;
    assert_eq!(
        outcome,
        RateLimitDecision::Throttled {
            retry_after_secs: 3
        },
    );
}

#[test]
fn parse_accepts_redis_int_in_payload() {
    // The Lua script emits the second slot as a bulk string today,
    // but a future refactor could return a Lua number which Redis
    // serialises as `Value::Int`. The parser must accept either.
    let value = RedisValue::Array(vec![bulk("allowed"), RedisValue::Int(5), bulk("requested")]);
    let outcome = parse_bounded_outcome(value).expect("parse").decision;
    assert_eq!(outcome, RateLimitDecision::Allowed { remaining: 5 });
}

#[test]
fn parse_rejects_unknown_tag() {
    let value = RedisValue::Array(vec![bulk("weird"), bulk("1"), bulk("requested")]);
    let err = parse_bounded_outcome(value).expect_err("must reject");
    assert!(matches!(err, CratestackError::Internal(_)));
}

#[test]
fn parse_rejects_non_array_root() {
    let err = parse_bounded_outcome(bulk("allowed")).expect_err("must reject");
    assert!(matches!(err, CratestackError::Internal(_)));
}

#[test]
fn parse_rejects_negative_remaining() {
    // The script clamps remaining to 0, but a buggy upstream could
    // send a negative value. Refuse to silently coerce.
    let value = RedisValue::Array(vec![bulk("allowed"), bulk("-1"), bulk("requested")]);
    let err = parse_bounded_outcome(value).expect_err("must reject");
    assert!(matches!(err, CratestackError::Internal(_)));
}

/// The `charged` slot is mandatory: a reply without it is a script we do
/// not recognise, and reporting `Requested` for what might have been a
/// fallback would make the cratestack#871 bound claim success it cannot
/// back up.
#[test]
fn parse_rejects_a_reply_missing_the_charged_slot() {
    let value = RedisValue::Array(vec![bulk("allowed"), bulk("7")]);
    let err = parse_bounded_outcome(value).expect_err("must reject");
    match err {
        CratestackError::Internal(msg) => assert!(msg.contains("charged"), "msg: {msg}"),
        other => panic!("expected Internal, got {other:?}"),
    }
}

#[test]
fn parse_reads_the_charged_slot() {
    let value = RedisValue::Array(vec![bulk("allowed"), bulk("7"), bulk("fallback")]);
    let outcome = parse_bounded_outcome(value).expect("parse");
    assert_eq!(outcome.charged, Charged::Fallback);

    let value = RedisValue::Array(vec![bulk("allowed"), bulk("7"), bulk("requested")]);
    let outcome = parse_bounded_outcome(value).expect("parse");
    assert_eq!(outcome.charged, Charged::Requested);
}

#[test]
fn parse_rejects_an_unknown_charged_tag() {
    let value = RedisValue::Array(vec![bulk("allowed"), bulk("7"), bulk("sideways")]);
    let err = parse_bounded_outcome(value).expect_err("must reject");
    assert!(matches!(err, CratestackError::Internal(_)));
}

#[test]
fn parse_rejects_truncated_array() {
    let value = RedisValue::Array(vec![bulk("allowed")]);
    let err = parse_bounded_outcome(value).expect_err("must reject");
    match err {
        CratestackError::Internal(msg) => assert!(msg.contains("missing"), "msg: {msg}"),
        other => panic!("expected Internal, got {other:?}"),
    }
}
