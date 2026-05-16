use cratestack_axum::ratelimit::RateLimitDecision;
use cratestack_core::CoolError;
use redis::Value as RedisValue;

pub(super) fn parse_consume_outcome(value: RedisValue) -> Result<RateLimitDecision, CoolError> {
    let items = match value {
        RedisValue::Array(items) => items,
        other => {
            return Err(CoolError::Internal(format!(
                "redis rate limit: expected array from consume script, got {other:?}"
            )));
        }
    };
    let mut iter = items.into_iter();
    let tag = next_string(&mut iter, "tag")?;
    match tag.as_str() {
        "allowed" => {
            let remaining = next_u32_decimal(&mut iter, "remaining")?;
            Ok(RateLimitDecision::Allowed { remaining })
        }
        "throttled" => {
            let retry_after_secs = next_u32_decimal(&mut iter, "retry_after_secs")?;
            Ok(RateLimitDecision::Throttled { retry_after_secs })
        }
        other => Err(CoolError::Internal(format!(
            "redis rate limit: unexpected outcome tag: {other}"
        ))),
    }
}

pub(super) fn next_string<I: Iterator<Item = RedisValue>>(
    iter: &mut I,
    field: &str,
) -> Result<String, CoolError> {
    let v = iter
        .next()
        .ok_or_else(|| CoolError::Internal(format!("redis rate limit: missing {field}")))?;
    match v {
        RedisValue::BulkString(b) => String::from_utf8(b).map_err(|err| {
            CoolError::Internal(format!("redis rate limit: {field} not utf8: {err}"))
        }),
        RedisValue::SimpleString(s) => Ok(s),
        other => Err(CoolError::Internal(format!(
            "redis rate limit: expected string for {field}, got {other:?}"
        ))),
    }
}

pub(super) fn next_i64_decimal<I: Iterator<Item = RedisValue>>(
    iter: &mut I,
    field: &str,
) -> Result<i64, CoolError> {
    let v = iter
        .next()
        .ok_or_else(|| CoolError::Internal(format!("redis rate limit: missing {field}")))?;
    let bytes = match v {
        RedisValue::Int(n) => return Ok(n),
        RedisValue::BulkString(b) => b,
        RedisValue::SimpleString(s) => s.into_bytes(),
        other => {
            return Err(CoolError::Internal(format!(
                "redis rate limit: expected number for {field}, got {other:?}"
            )));
        }
    };
    std::str::from_utf8(&bytes)
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .ok_or_else(|| CoolError::Internal(format!("redis rate limit: bad number for {field}")))
}

pub(super) fn next_u32_decimal<I: Iterator<Item = RedisValue>>(
    iter: &mut I,
    field: &str,
) -> Result<u32, CoolError> {
    let n = next_i64_decimal(iter, field)?;
    u32::try_from(n).map_err(|_| {
        CoolError::Internal(format!(
            "redis rate limit: {field} out of u32 range: {n}"
        ))
    })
}
