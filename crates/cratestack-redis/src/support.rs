//! Shared scaffolding for [`crate::idempotency`] and [`crate::ratelimit`].
//!
//! Both stores hash caller-supplied identifiers into a 64-char hex suffix
//! attached to a configurable key prefix, run a Lua script over a
//! `MultiplexedConnection`, and decode the same handful of Redis reply
//! shapes (bulk strings, simple strings, ints, optional bytes). Pulling
//! the common parts here keeps the per-store modules focused on their
//! Lua + outcome parser.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cratestack_core::CoolError;
use redis::Value as RedisValue;
use sha2::{Digest, Sha256};

/// Configured key namespace shared by both stores. The prefix is normalised
/// once on construction; `hashed_key` then derives a bounded, escaping-safe
/// Redis key for any caller-supplied input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KeyNamespace {
    prefix: String,
    segment: &'static str,
}

impl KeyNamespace {
    pub(crate) fn new(prefix: impl Into<String>, segment: &'static str) -> Self {
        Self {
            prefix: normalize_key_prefix(prefix.into()),
            segment,
        }
    }

    pub(crate) fn prefix(&self) -> &str {
        &self.prefix
    }

    /// Compose the Redis key `<prefix>:<segment>:<sha256_hex(input)>`.
    /// Each input slice is hashed with a `0x00` separator so concatenated
    /// callers (e.g. `principal || key`) can't collide on boundary
    /// ambiguity.
    pub(crate) fn hashed_key(&self, parts: &[&[u8]]) -> String {
        let mut hasher = Sha256::new();
        for (idx, part) in parts.iter().enumerate() {
            if idx > 0 {
                hasher.update([0u8]);
            }
            hasher.update(part);
        }
        let digest = hasher.finalize();
        let mut out = String::with_capacity(self.prefix.len() + self.segment.len() + 2 + 64);
        out.push_str(&self.prefix);
        out.push(':');
        out.push_str(self.segment);
        out.push(':');
        for byte in digest {
            out.push(nibble_hex(byte >> 4));
            out.push(nibble_hex(byte & 0x0f));
        }
        out
    }
}

/// Strip outer whitespace/colon noise from a configured prefix and fall
/// back to a stable default. The order
/// `trim → trim_matches(':') → trim` matters for inputs like `" : : "`
/// where each pass exposes more noise to the next.
pub(crate) fn normalize_key_prefix(key_prefix: String) -> String {
    let cleaned = key_prefix.trim().trim_matches(':').trim();
    if cleaned.is_empty() {
        "cratestack".to_owned()
    } else {
        cleaned.to_owned()
    }
}

pub(crate) fn nibble_hex(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'a' + nibble - 10) as char,
        _ => unreachable!("nibble must be 0..=15"),
    }
}

/// Wrap a `redis::RedisError` into the right `CoolError::Internal` with a
/// per-store scope tag so operators can pinpoint which subsystem failed.
pub(crate) fn redis_error(scope: &'static str, error: redis::RedisError) -> CoolError {
    CoolError::Internal(format!("{scope}: {error}"))
}

pub(crate) fn system_time_to_ms(scope: &'static str, time: SystemTime) -> Result<i64, CoolError> {
    let dur = time.duration_since(UNIX_EPOCH).map_err(|err| {
        CoolError::Internal(format!("{scope}: timestamp before unix epoch: {err}"))
    })?;
    i64::try_from(dur.as_millis())
        .map_err(|_| CoolError::Internal(format!("{scope}: timestamp out of i64 ms range")))
}

pub(crate) fn system_time_from_ms(ms: i64) -> SystemTime {
    if ms >= 0 {
        UNIX_EPOCH + Duration::from_millis(ms as u64)
    } else {
        UNIX_EPOCH - Duration::from_millis(ms.unsigned_abs())
    }
}

pub(crate) fn value_as_string(value: &RedisValue) -> Option<String> {
    match value {
        RedisValue::SimpleString(s) => Some(s.clone()),
        RedisValue::BulkString(b) => std::str::from_utf8(b).ok().map(str::to_owned),
        RedisValue::Okay => Some("OK".to_owned()),
        _ => None,
    }
}

pub(crate) fn next_string<I: Iterator<Item = RedisValue>>(
    scope: &'static str,
    iter: &mut I,
    field: &str,
) -> Result<String, CoolError> {
    let v = iter
        .next()
        .ok_or_else(|| CoolError::Internal(format!("{scope}: missing {field}")))?;
    match v {
        RedisValue::BulkString(b) => String::from_utf8(b)
            .map_err(|err| CoolError::Internal(format!("{scope}: {field} not utf8: {err}"))),
        RedisValue::SimpleString(s) => Ok(s),
        other => Err(CoolError::Internal(format!(
            "{scope}: expected string for {field}, got {other:?}"
        ))),
    }
}

pub(crate) fn next_bytes<I: Iterator<Item = RedisValue>>(
    scope: &'static str,
    iter: &mut I,
    field: &str,
) -> Result<Vec<u8>, CoolError> {
    let v = iter
        .next()
        .ok_or_else(|| CoolError::Internal(format!("{scope}: missing {field}")))?;
    match v {
        RedisValue::BulkString(b) => Ok(b),
        RedisValue::SimpleString(s) => Ok(s.into_bytes()),
        RedisValue::Nil => Ok(Vec::new()),
        other => Err(CoolError::Internal(format!(
            "{scope}: expected bytes for {field}, got {other:?}"
        ))),
    }
}

pub(crate) fn next_i64_decimal<I: Iterator<Item = RedisValue>>(
    scope: &'static str,
    iter: &mut I,
    field: &str,
) -> Result<i64, CoolError> {
    let v = iter
        .next()
        .ok_or_else(|| CoolError::Internal(format!("{scope}: missing {field}")))?;
    let bytes = match v {
        RedisValue::Int(n) => return Ok(n),
        RedisValue::BulkString(b) => b,
        RedisValue::SimpleString(s) => s.into_bytes(),
        other => {
            return Err(CoolError::Internal(format!(
                "{scope}: expected number for {field}, got {other:?}"
            )));
        }
    };
    std::str::from_utf8(&bytes)
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .ok_or_else(|| CoolError::Internal(format!("{scope}: bad number for {field}")))
}

pub(crate) fn next_u16_decimal<I: Iterator<Item = RedisValue>>(
    scope: &'static str,
    iter: &mut I,
    field: &str,
) -> Result<u16, CoolError> {
    let n = next_i64_decimal(scope, iter, field)?;
    u16::try_from(n)
        .map_err(|_| CoolError::Internal(format!("{scope}: {field} out of u16 range: {n}")))
}

pub(crate) fn next_u32_decimal<I: Iterator<Item = RedisValue>>(
    scope: &'static str,
    iter: &mut I,
    field: &str,
) -> Result<u32, CoolError> {
    let n = next_i64_decimal(scope, iter, field)?;
    u32::try_from(n)
        .map_err(|_| CoolError::Internal(format!("{scope}: {field} out of u32 range: {n}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_outer_colons_and_whitespace() {
        assert_eq!(normalize_key_prefix(":bank:idem:".into()), "bank:idem");
        assert_eq!(normalize_key_prefix("  bank  ".into()), "bank");
    }

    #[test]
    fn normalize_falls_back_to_default_when_empty() {
        for input in ["", "::", ":::", "   ", " : : "] {
            assert_eq!(normalize_key_prefix(input.into()), "cratestack");
        }
    }

    #[test]
    fn normalize_preserves_inner_colons() {
        assert_eq!(
            normalize_key_prefix("bank:au:idem".into()),
            "bank:au:idem"
        );
    }

    #[test]
    fn nibble_hex_covers_all_nibbles() {
        let expected = "0123456789abcdef";
        for (n, ch) in expected.chars().enumerate() {
            assert_eq!(nibble_hex(n as u8), ch);
        }
    }

    #[test]
    fn key_namespace_uses_configured_segment_and_64_hex_chars() {
        let ns = KeyNamespace::new("bank", "idem");
        let key = ns.hashed_key(&[b"alice", b"txn-1"]);
        let suffix = key
            .strip_prefix("bank:idem:")
            .expect("namespace must use `<prefix>:<segment>:` layout");
        assert_eq!(suffix.len(), 64);
        assert!(
            suffix
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
    }

    #[test]
    fn key_namespace_disambiguates_boundary_collisions() {
        let ns = KeyNamespace::new("bank", "idem");
        // ("ab", "c") and ("a", "bc") must not collide — the 0x00
        // separator inside hashed_key prevents naive concatenation
        // ambiguity.
        assert_ne!(ns.hashed_key(&[b"ab", b"c"]), ns.hashed_key(&[b"a", b"bc"]));
    }

    #[test]
    fn key_namespace_distinguishes_different_prefixes() {
        let staging = KeyNamespace::new("staging", "idem");
        let prod = KeyNamespace::new("prod", "idem");
        assert_ne!(staging.hashed_key(&[b"k"]), prod.hashed_key(&[b"k"]));
    }

    #[test]
    fn system_time_roundtrip_near_now() {
        let now = SystemTime::now();
        let ms = system_time_to_ms("test", now).unwrap();
        let back = system_time_from_ms(ms);
        let drift = now
            .duration_since(back)
            .or_else(|err| Ok::<_, std::time::SystemTimeError>(err.duration()))
            .unwrap();
        assert!(drift < Duration::from_millis(2));
    }

    #[test]
    fn system_time_rejects_pre_epoch() {
        let before = UNIX_EPOCH - Duration::from_secs(1);
        assert!(system_time_to_ms("test", before).is_err());
    }
}
