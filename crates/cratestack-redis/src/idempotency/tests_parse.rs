#![cfg(test)]

use cratestack_axum::idempotency::ReservationOutcome;
use cratestack_core::CoolError;
use redis::Value as RedisValue;

use super::parse::parse_reserve_outcome;
use super::tests_fixtures::{bulk, raw_bulk};
use super::time::system_time_to_ms;

#[test]
fn parse_reserved_extracts_token_bytes() {
    let token = uuid::Uuid::new_v4();
    let value = RedisValue::Array(vec![bulk("reserved"), raw_bulk(token.as_bytes())]);
    let outcome = parse_reserve_outcome(value, "p", "k").expect("parse should succeed");
    match outcome {
        ReservationOutcome::Reserved { token: got } => assert_eq!(got, token),
        other => panic!("expected Reserved, got {other:?}"),
    }
}

#[test]
fn parse_reserved_rejects_wrong_length_token() {
    // 8 bytes is too short for a UUID — Uuid::from_slice errors,
    // which we surface as Internal rather than panicking.
    let value = RedisValue::Array(vec![bulk("reserved"), raw_bulk([0u8; 8])]);
    let err = parse_reserve_outcome(value, "p", "k").expect_err("must reject short token");
    assert!(matches!(err, CoolError::Internal(_)));
}

#[test]
fn parse_in_flight_returns_in_flight() {
    let value = RedisValue::Array(vec![bulk("in_flight")]);
    let outcome = parse_reserve_outcome(value, "p", "k").expect("parse");
    assert!(matches!(outcome, ReservationOutcome::InFlight));
}

#[test]
fn parse_conflict_returns_conflict() {
    let value = RedisValue::Array(vec![bulk("conflict")]);
    let outcome = parse_reserve_outcome(value, "p", "k").expect("parse");
    assert!(matches!(outcome, ReservationOutcome::Conflict));
}

#[test]
fn parse_replay_reconstructs_record_exactly() {
    let hash = [9u8; 32];
    let created_ms = 1_700_000_000_000i64;
    let expires_ms = 1_700_000_060_000i64;
    let headers = b"content-type:application/json\n";
    let body = br#"{"transfer_id":"abc"}"#;
    let value = RedisValue::Array(vec![
        bulk("replay"),
        raw_bulk(hash),
        bulk("201"),
        raw_bulk(headers),
        raw_bulk(body),
        bulk(&created_ms.to_string()),
        bulk(&expires_ms.to_string()),
    ]);
    let outcome = parse_reserve_outcome(value, "fp", "k").expect("parse");
    let record = match outcome {
        ReservationOutcome::Replay(r) => r,
        other => panic!("expected Replay, got {other:?}"),
    };
    assert_eq!(record.principal_fingerprint, "fp");
    assert_eq!(record.key, "k");
    assert_eq!(record.request_hash, hash);
    assert_eq!(record.response_status, 201);
    assert_eq!(record.response_headers, headers);
    assert_eq!(record.response_body, body);
    assert_eq!(system_time_to_ms(record.created_at).unwrap(), created_ms);
    assert_eq!(system_time_to_ms(record.expires_at).unwrap(), expires_ms);
}

#[test]
fn parse_replay_tolerates_empty_headers_and_body() {
    // A response with no headers / empty body is legal — make sure
    // the parser doesn't reject Nil or empty BulkString fields.
    let value = RedisValue::Array(vec![
        bulk("replay"),
        raw_bulk([0u8; 32]),
        bulk("204"),
        RedisValue::Nil,
        RedisValue::BulkString(Vec::new()),
        bulk("0"),
        bulk("0"),
    ]);
    let record = match parse_reserve_outcome(value, "p", "k").expect("parse") {
        ReservationOutcome::Replay(r) => r,
        other => panic!("expected Replay, got {other:?}"),
    };
    assert_eq!(record.response_status, 204);
    assert!(record.response_headers.is_empty());
    assert!(record.response_body.is_empty());
}

#[test]
fn parse_replay_rejects_hash_with_wrong_length() {
    let value = RedisValue::Array(vec![
        bulk("replay"),
        raw_bulk([0u8; 16]), // wrong length
        bulk("200"),
        raw_bulk([]),
        raw_bulk([]),
        bulk("0"),
        bulk("0"),
    ]);
    let err = parse_reserve_outcome(value, "p", "k").expect_err("must reject");
    match err {
        CoolError::Internal(msg) => assert!(msg.contains("wrong length"), "msg: {msg}"),
        other => panic!("expected Internal, got {other:?}"),
    }
}

#[test]
fn parse_replay_rejects_out_of_range_status() {
    // Status 70000 doesn't fit u16. The script shouldn't produce
    // this, but the parser still has to refuse rather than silently
    // truncate.
    let value = RedisValue::Array(vec![
        bulk("replay"),
        raw_bulk([0u8; 32]),
        bulk("70000"),
        raw_bulk([]),
        raw_bulk([]),
        bulk("0"),
        bulk("0"),
    ]);
    let err = parse_reserve_outcome(value, "p", "k").expect_err("must reject");
    assert!(matches!(err, CoolError::Internal(_)));
}

#[test]
fn parse_replay_rejects_non_numeric_status() {
    let value = RedisValue::Array(vec![
        bulk("replay"),
        raw_bulk([0u8; 32]),
        bulk("not-a-number"),
        raw_bulk([]),
        raw_bulk([]),
        bulk("0"),
        bulk("0"),
    ]);
    let err = parse_reserve_outcome(value, "p", "k").expect_err("must reject");
    assert!(matches!(err, CoolError::Internal(_)));
}

#[test]
fn parse_rejects_unknown_tag() {
    let value = RedisValue::Array(vec![bulk("weird")]);
    let err = parse_reserve_outcome(value, "p", "k").expect_err("must reject");
    assert!(matches!(err, CoolError::Internal(_)));
}

#[test]
fn parse_rejects_non_array_root() {
    // The reserve script always returns a Lua table, which Redis
    // serialises as a multi-bulk reply (`Value::Array`). Anything
    // else is corruption — refuse rather than guess.
    let err =
        parse_reserve_outcome(bulk("reserved"), "p", "k").expect_err("non-array root must error");
    assert!(matches!(err, CoolError::Internal(_)));
}

#[test]
fn parse_replay_with_truncated_array_errors() {
    // Missing fields after "replay" — happens if a future Lua
    // refactor forgets a field. The parser must report a clear
    // "missing X" rather than silently using defaults.
    let value = RedisValue::Array(vec![bulk("replay"), raw_bulk([0u8; 32])]);
    let err = parse_reserve_outcome(value, "p", "k").expect_err("must reject");
    match err {
        CoolError::Internal(msg) => assert!(msg.contains("missing"), "msg: {msg}"),
        other => panic!("expected Internal, got {other:?}"),
    }
}
