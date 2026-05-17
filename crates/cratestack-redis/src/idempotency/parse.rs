use cratestack_axum::idempotency::{IdempotencyRecord, ReservationOutcome};
use cratestack_core::CoolError;
use redis::Value as RedisValue;
use uuid::Uuid;

use super::time::system_time_from_ms;

pub(super) fn parse_reserve_outcome(
    value: RedisValue,
    principal: &str,
    key: &str,
) -> Result<ReservationOutcome, CoolError> {
    let items = match value {
        RedisValue::Array(items) => items,
        other => {
            return Err(CoolError::Internal(format!(
                "redis idempotency: expected array from reserve script, got {other:?}"
            )));
        }
    };
    let mut iter = items.into_iter();
    let tag = next_string(&mut iter, "tag")?;
    match tag.as_str() {
        "reserved" => {
            let token_bytes = next_bytes(&mut iter, "token")?;
            let token = Uuid::from_slice(&token_bytes).map_err(|err| {
                CoolError::Internal(format!("redis idempotency: bad token bytes: {err}"))
            })?;
            Ok(ReservationOutcome::Reserved { token })
        }
        "conflict" => Ok(ReservationOutcome::Conflict),
        "in_flight" => Ok(ReservationOutcome::InFlight),
        "replay" => {
            let hash_bytes = next_bytes(&mut iter, "request_hash")?;
            let request_hash: [u8; 32] = hash_bytes.as_slice().try_into().map_err(|_| {
                CoolError::Internal("redis idempotency: stored hash has wrong length".to_owned())
            })?;
            let response_status = next_u16_decimal(&mut iter, "response_status")?;
            let response_headers = next_bytes(&mut iter, "response_headers")?;
            let response_body = next_bytes(&mut iter, "response_body")?;
            let created_ms = next_i64_decimal(&mut iter, "created_at")?;
            let expires_ms = next_i64_decimal(&mut iter, "expires_at")?;
            Ok(ReservationOutcome::Replay(IdempotencyRecord {
                principal_fingerprint: principal.to_owned(),
                key: key.to_owned(),
                request_hash,
                response_status,
                response_headers,
                response_body,
                created_at: system_time_from_ms(created_ms),
                expires_at: system_time_from_ms(expires_ms),
            }))
        }
        other => Err(CoolError::Internal(format!(
            "redis idempotency: unexpected outcome tag: {other}"
        ))),
    }
}

pub(super) fn next_string<I: Iterator<Item = RedisValue>>(
    iter: &mut I,
    field: &str,
) -> Result<String, CoolError> {
    let v = iter
        .next()
        .ok_or_else(|| CoolError::Internal(format!("redis idempotency: missing {field}")))?;
    match v {
        RedisValue::BulkString(b) => String::from_utf8(b).map_err(|err| {
            CoolError::Internal(format!("redis idempotency: {field} not utf8: {err}"))
        }),
        RedisValue::SimpleString(s) => Ok(s),
        other => Err(CoolError::Internal(format!(
            "redis idempotency: expected string for {field}, got {other:?}"
        ))),
    }
}

pub(super) fn next_bytes<I: Iterator<Item = RedisValue>>(
    iter: &mut I,
    field: &str,
) -> Result<Vec<u8>, CoolError> {
    let v = iter
        .next()
        .ok_or_else(|| CoolError::Internal(format!("redis idempotency: missing {field}")))?;
    match v {
        RedisValue::BulkString(b) => Ok(b),
        RedisValue::SimpleString(s) => Ok(s.into_bytes()),
        RedisValue::Nil => Ok(Vec::new()),
        other => Err(CoolError::Internal(format!(
            "redis idempotency: expected bytes for {field}, got {other:?}"
        ))),
    }
}

pub(super) fn next_i64_decimal<I: Iterator<Item = RedisValue>>(
    iter: &mut I,
    field: &str,
) -> Result<i64, CoolError> {
    let v = iter
        .next()
        .ok_or_else(|| CoolError::Internal(format!("redis idempotency: missing {field}")))?;
    let bytes = match v {
        RedisValue::Int(n) => return Ok(n),
        RedisValue::BulkString(b) => b,
        RedisValue::SimpleString(s) => s.into_bytes(),
        other => {
            return Err(CoolError::Internal(format!(
                "redis idempotency: expected number for {field}, got {other:?}"
            )));
        }
    };
    std::str::from_utf8(&bytes)
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .ok_or_else(|| CoolError::Internal(format!("redis idempotency: bad number for {field}")))
}

pub(super) fn next_u16_decimal<I: Iterator<Item = RedisValue>>(
    iter: &mut I,
    field: &str,
) -> Result<u16, CoolError> {
    let n = next_i64_decimal(iter, field)?;
    u16::try_from(n).map_err(|_| {
        CoolError::Internal(format!("redis idempotency: {field} out of u16 range: {n}"))
    })
}

pub(super) fn value_as_string(value: &RedisValue) -> Option<String> {
    match value {
        RedisValue::SimpleString(s) => Some(s.clone()),
        RedisValue::BulkString(b) => std::str::from_utf8(b).ok().map(str::to_owned),
        RedisValue::Okay => Some("OK".to_owned()),
        _ => None,
    }
}
