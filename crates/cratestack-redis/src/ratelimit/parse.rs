use cratestack_core::{BoundedOutcome, Charged, CratestackError, RateLimitDecision};
use redis::Value as RedisValue;

/// Parses the script's `{tag, value, charged}` reply.
///
/// The third slot is **required**, not optional-with-a-default: the only
/// producer is `super::scripts::CONSUME_LUA`, which always emits it, so a
/// two-slot reply means we are talking to a script we do not recognise.
/// Defaulting there would silently report `Requested` for a charge that
/// may have been a fallback — a bound that reports itself as working when
/// it is not is worse than one that errors (cratestack#871).
pub(super) fn parse_bounded_outcome(value: RedisValue) -> Result<BoundedOutcome, CratestackError> {
    let items = match value {
        RedisValue::Array(items) => items,
        other => {
            return Err(CratestackError::Internal(format!(
                "redis rate limit: expected array from consume script, got {other:?}"
            )));
        }
    };
    let mut iter = items.into_iter();
    let tag = next_string(&mut iter, "tag")?;
    let decision = match tag.as_str() {
        "allowed" => {
            let remaining = next_u32_decimal(&mut iter, "remaining")?;
            RateLimitDecision::Allowed { remaining }
        }
        "throttled" => {
            let retry_after_secs = next_u32_decimal(&mut iter, "retry_after_secs")?;
            RateLimitDecision::Throttled { retry_after_secs }
        }
        other => {
            return Err(CratestackError::Internal(format!(
                "redis rate limit: unexpected outcome tag: {other}"
            )));
        }
    };
    let charged = match next_string(&mut iter, "charged")?.as_str() {
        "requested" => Charged::Requested,
        // The script cannot tell a per-peer scope from the process-global
        // one and does not need to; `cratestack_axum` refines this into
        // `Charged::Overflow` where that distinction is known.
        "fallback" => Charged::Fallback,
        other => {
            return Err(CratestackError::Internal(format!(
                "redis rate limit: unexpected charged tag: {other}"
            )));
        }
    };
    Ok(BoundedOutcome::new(decision, charged))
}

pub(super) fn next_string<I: Iterator<Item = RedisValue>>(
    iter: &mut I,
    field: &str,
) -> Result<String, CratestackError> {
    let v = iter
        .next()
        .ok_or_else(|| CratestackError::Internal(format!("redis rate limit: missing {field}")))?;
    match v {
        RedisValue::BulkString(b) => String::from_utf8(b).map_err(|err| {
            CratestackError::Internal(format!("redis rate limit: {field} not utf8: {err}"))
        }),
        RedisValue::SimpleString(s) => Ok(s),
        other => Err(CratestackError::Internal(format!(
            "redis rate limit: expected string for {field}, got {other:?}"
        ))),
    }
}

pub(super) fn next_i64_decimal<I: Iterator<Item = RedisValue>>(
    iter: &mut I,
    field: &str,
) -> Result<i64, CratestackError> {
    let v = iter
        .next()
        .ok_or_else(|| CratestackError::Internal(format!("redis rate limit: missing {field}")))?;
    let bytes = match v {
        RedisValue::Int(n) => return Ok(n),
        RedisValue::BulkString(b) => b,
        RedisValue::SimpleString(s) => s.into_bytes(),
        other => {
            return Err(CratestackError::Internal(format!(
                "redis rate limit: expected number for {field}, got {other:?}"
            )));
        }
    };
    std::str::from_utf8(&bytes)
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .ok_or_else(|| {
            CratestackError::Internal(format!("redis rate limit: bad number for {field}"))
        })
}

pub(super) fn next_u32_decimal<I: Iterator<Item = RedisValue>>(
    iter: &mut I,
    field: &str,
) -> Result<u32, CratestackError> {
    let n = next_i64_decimal(iter, field)?;
    u32::try_from(n).map_err(|_| {
        CratestackError::Internal(format!("redis rate limit: {field} out of u32 range: {n}"))
    })
}
