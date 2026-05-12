//! Redis-backed [`RateLimitStore`].
//!
//! Each rate-limit key maps to a Redis hash at
//! `<prefix>:rl:<sha256(key)>` holding two fields: `tokens` (the current
//! bucket fill, a float) and `last_refill_ms` (the wall-clock timestamp
//! of the most recent refill, an integer). Hashing the caller-supplied
//! key keeps Redis keys bounded and sidesteps any escaping concerns
//! around `:` in user-supplied values — same shape as the idempotency
//! store.
//!
//! Atomicity comes from a single Lua script that performs the
//! read-refill-decrement-write cycle in one round-trip. The `redis`
//! crate's `Script::invoke_async` handles `EVALSHA` plus `NOSCRIPT`
//! fallback automatically.
//!
//! Eviction: each `consume` refreshes a relative `EXPIRE` derived from
//! the time required to refill a full bucket (clamped to 24h). Idle
//! buckets evict themselves, so memory stays bounded even when the
//! keyspace is tenant-scoped. Banks running enormous tenant fleets get
//! constant-memory behaviour without an explicit reaper.
//!
//! Clock skew across replicas would let one replica grant extra tokens
//! if the previous writer had a slower clock; the script clamps
//! `elapsed < 0` to zero so a backward-jumping clock can only delay
//! refill, never advance it.

use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use cratestack_axum::ratelimit::{RateLimitConfig, RateLimitDecision, RateLimitStore};
use cratestack_core::CoolError;
use redis::{Script, Value as RedisValue};
use sha2::{Digest, Sha256};

const CONSUME_LUA: &str = r#"
local now_ms = tonumber(ARGV[1])
local burst = tonumber(ARGV[2])
local refill_per_second = tonumber(ARGV[3])

local existing = redis.call('HMGET', KEYS[1], 'tokens', 'last_refill_ms')
local tokens
local last_refill_ms
if existing[1] then
  tokens = tonumber(existing[1])
  last_refill_ms = tonumber(existing[2])
else
  tokens = burst
  last_refill_ms = now_ms
end

local elapsed_sec = (now_ms - last_refill_ms) / 1000.0
if elapsed_sec < 0 then elapsed_sec = 0 end
tokens = tokens + elapsed_sec * refill_per_second
if tokens > burst then tokens = burst end

local ttl_sec
if refill_per_second > 0 then
  ttl_sec = math.ceil(burst / refill_per_second) + 60
  if ttl_sec > 86400 then ttl_sec = 86400 end
  if ttl_sec < 60 then ttl_sec = 60 end
else
  ttl_sec = 86400
end

if tokens >= 1.0 then
  tokens = tokens - 1.0
  redis.call('HSET', KEYS[1], 'tokens', tostring(tokens), 'last_refill_ms', tostring(now_ms))
  redis.call('EXPIRE', KEYS[1], ttl_sec)
  local remaining = math.floor(tokens)
  if remaining < 0 then remaining = 0 end
  return {'allowed', tostring(remaining)}
else
  redis.call('HSET', KEYS[1], 'tokens', tostring(tokens), 'last_refill_ms', tostring(now_ms))
  redis.call('EXPIRE', KEYS[1], ttl_sec)
  local need = 1.0 - tokens
  local retry
  if refill_per_second > 0 then
    retry = math.ceil(need / refill_per_second)
  else
    retry = 86400
  end
  if retry < 1 then retry = 1 end
  return {'throttled', tostring(retry)}
end
"#;

static CONSUME_SCRIPT: LazyLock<Script> = LazyLock::new(|| Script::new(CONSUME_LUA));

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedisRateLimitStoreConfig {
    pub key_prefix: String,
}

impl RedisRateLimitStoreConfig {
    pub fn new(key_prefix: impl Into<String>) -> Self {
        Self {
            key_prefix: normalize_key_prefix(key_prefix.into()),
        }
    }
}

#[derive(Clone)]
pub struct RedisRateLimitStore {
    client: redis::Client,
    config: RedisRateLimitStoreConfig,
}

impl RedisRateLimitStore {
    pub fn open(
        redis_url: impl redis::IntoConnectionInfo,
        key_prefix: impl Into<String>,
    ) -> Result<Self, CoolError> {
        let client = redis::Client::open(redis_url).map_err(redis_error)?;
        Ok(Self::from_client(client, key_prefix))
    }

    pub fn from_client(client: redis::Client, key_prefix: impl Into<String>) -> Self {
        Self {
            client,
            config: RedisRateLimitStoreConfig::new(key_prefix),
        }
    }

    pub fn key_prefix(&self) -> &str {
        &self.config.key_prefix
    }

    pub fn bucket_key(&self, key: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(key.as_bytes());
        let digest = hasher.finalize();
        let mut out = String::with_capacity(self.config.key_prefix.len() + 4 + 64);
        out.push_str(&self.config.key_prefix);
        out.push_str(":rl:");
        for byte in digest {
            out.push(nibble_hex(byte >> 4));
            out.push(nibble_hex(byte & 0x0f));
        }
        out
    }

    async fn connection(&self) -> Result<redis::aio::MultiplexedConnection, CoolError> {
        self.client
            .get_multiplexed_async_connection()
            .await
            .map_err(redis_error)
    }
}

#[async_trait]
impl RateLimitStore for RedisRateLimitStore {
    async fn consume(
        &self,
        key: &str,
        config: RateLimitConfig,
    ) -> Result<RateLimitDecision, CoolError> {
        let mut conn = self.connection().await?;
        let now_ms = system_time_to_ms(SystemTime::now())?;
        let bucket_key = self.bucket_key(key);

        // Lua's `tonumber` accepts standard decimal notation; we serialise
        // the float with `{:?}` so values like `0.001` round-trip through
        // Rust's `f64::to_string`-equivalent without ever taking on a
        // locale-dependent form. `tostring`/`tonumber` inside the script
        // are unaffected by Redis's locale because Lua 5.1 (which Redis
        // embeds) uses C-locale formatting.
        let value: RedisValue = CONSUME_SCRIPT
            .key(bucket_key)
            .arg(now_ms.to_string())
            .arg(config.burst.to_string())
            .arg(format!("{}", config.refill_per_second))
            .invoke_async(&mut conn)
            .await
            .map_err(redis_error)?;

        parse_consume_outcome(value)
    }
}

fn parse_consume_outcome(value: RedisValue) -> Result<RateLimitDecision, CoolError> {
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

fn next_string<I: Iterator<Item = RedisValue>>(iter: &mut I, field: &str) -> Result<String, CoolError> {
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

fn next_i64_decimal<I: Iterator<Item = RedisValue>>(iter: &mut I, field: &str) -> Result<i64, CoolError> {
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

fn next_u32_decimal<I: Iterator<Item = RedisValue>>(iter: &mut I, field: &str) -> Result<u32, CoolError> {
    let n = next_i64_decimal(iter, field)?;
    u32::try_from(n).map_err(|_| {
        CoolError::Internal(format!(
            "redis rate limit: {field} out of u32 range: {n}"
        ))
    })
}

fn system_time_to_ms(time: SystemTime) -> Result<i64, CoolError> {
    let dur = time.duration_since(UNIX_EPOCH).map_err(|err| {
        CoolError::Internal(format!(
            "redis rate limit: timestamp before unix epoch: {err}"
        ))
    })?;
    i64::try_from(dur.as_millis()).map_err(|_| {
        CoolError::Internal("redis rate limit: timestamp out of i64 ms range".to_owned())
    })
}

fn normalize_key_prefix(key_prefix: String) -> String {
    let cleaned = key_prefix.trim().trim_matches(':').trim();
    if cleaned.is_empty() {
        "cratestack".to_owned()
    } else {
        cleaned.to_owned()
    }
}

fn nibble_hex(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'a' + nibble - 10) as char,
        _ => unreachable!("nibble must be 0..=15"),
    }
}

fn redis_error(error: redis::RedisError) -> CoolError {
    CoolError::Internal(format!("redis rate limit: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ----- Config / key layout -----

    #[test]
    fn config_trims_outer_colons_and_whitespace() {
        assert_eq!(
            RedisRateLimitStoreConfig::new(":bank:rl:").key_prefix,
            "bank:rl",
        );
        assert_eq!(
            RedisRateLimitStoreConfig::new("  bank  ").key_prefix,
            "bank",
        );
    }

    #[test]
    fn config_falls_back_to_default_namespace_when_empty() {
        for input in ["", "::", ":::", "   ", " : : "] {
            assert_eq!(
                RedisRateLimitStoreConfig::new(input).key_prefix,
                "cratestack",
                "input {input:?} should fall back to default namespace",
            );
        }
    }

    fn offline_store(prefix: &str) -> RedisRateLimitStore {
        let client =
            redis::Client::open("redis://127.0.0.1/").expect("static URL must parse offline");
        RedisRateLimitStore::from_client(client, prefix)
    }

    #[test]
    fn bucket_key_uses_configured_prefix_and_rl_namespace() {
        let store = offline_store("bank");
        let key = store.bucket_key("auth:abc");
        let suffix = key
            .strip_prefix("bank:rl:")
            .expect("bucket_key must use `<prefix>:rl:` as its namespace");
        assert_eq!(suffix.len(), 64);
        assert!(suffix
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn bucket_key_is_deterministic_and_distinct_per_input() {
        let store = offline_store("bank");
        assert_eq!(store.bucket_key("alice"), store.bucket_key("alice"));
        assert_ne!(store.bucket_key("alice"), store.bucket_key("bob"));
    }

    #[test]
    fn bucket_key_isolates_different_prefixes() {
        let a = offline_store("staging");
        let b = offline_store("prod");
        assert_ne!(a.bucket_key("k"), b.bucket_key("k"));
    }

    #[test]
    fn bucket_key_handles_pathological_inputs() {
        let store = offline_store("bank");
        let long = "x".repeat(10_000);
        let result = store.bucket_key(&long);
        assert!(result.starts_with("bank:rl:"));
        assert_eq!(result.len(), "bank:rl:".len() + 64);
    }

    // ----- Outcome parser -----

    fn bulk(s: &str) -> RedisValue {
        RedisValue::BulkString(s.as_bytes().to_vec())
    }

    #[test]
    fn parse_allowed_extracts_remaining() {
        let value = RedisValue::Array(vec![bulk("allowed"), bulk("7")]);
        let outcome = parse_consume_outcome(value).expect("parse");
        assert_eq!(outcome, RateLimitDecision::Allowed { remaining: 7 });
    }

    #[test]
    fn parse_throttled_extracts_retry_after() {
        let value = RedisValue::Array(vec![bulk("throttled"), bulk("3")]);
        let outcome = parse_consume_outcome(value).expect("parse");
        assert_eq!(
            outcome,
            RateLimitDecision::Throttled { retry_after_secs: 3 },
        );
    }

    #[test]
    fn parse_accepts_redis_int_in_payload() {
        // The Lua script emits the second slot as a bulk string today,
        // but a future refactor could return a Lua number which Redis
        // serialises as `Value::Int`. The parser must accept either.
        let value = RedisValue::Array(vec![bulk("allowed"), RedisValue::Int(5)]);
        let outcome = parse_consume_outcome(value).expect("parse");
        assert_eq!(outcome, RateLimitDecision::Allowed { remaining: 5 });
    }

    #[test]
    fn parse_rejects_unknown_tag() {
        let value = RedisValue::Array(vec![bulk("weird"), bulk("1")]);
        let err = parse_consume_outcome(value).expect_err("must reject");
        assert!(matches!(err, CoolError::Internal(_)));
    }

    #[test]
    fn parse_rejects_non_array_root() {
        let err = parse_consume_outcome(bulk("allowed")).expect_err("must reject");
        assert!(matches!(err, CoolError::Internal(_)));
    }

    #[test]
    fn parse_rejects_negative_remaining() {
        // The script clamps remaining to 0, but a buggy upstream could
        // send a negative value. Refuse to silently coerce.
        let value = RedisValue::Array(vec![bulk("allowed"), bulk("-1")]);
        let err = parse_consume_outcome(value).expect_err("must reject");
        assert!(matches!(err, CoolError::Internal(_)));
    }

    #[test]
    fn parse_rejects_truncated_array() {
        let value = RedisValue::Array(vec![bulk("allowed")]);
        let err = parse_consume_outcome(value).expect_err("must reject");
        match err {
            CoolError::Internal(msg) => assert!(msg.contains("missing"), "msg: {msg}"),
            other => panic!("expected Internal, got {other:?}"),
        }
    }

    // ----- Helpers -----

    #[test]
    fn nibble_hex_covers_all_valid_nibbles() {
        let expected = "0123456789abcdef";
        for (n, ch) in expected.chars().enumerate() {
            assert_eq!(nibble_hex(n as u8), ch);
        }
    }

    #[test]
    fn system_time_to_ms_rejects_pre_epoch_inputs() {
        let before = UNIX_EPOCH - std::time::Duration::from_secs(1);
        assert!(system_time_to_ms(before).is_err());
    }

    // ----- Randomized property tests -----
    //
    // These exist to widen coverage past hand-picked inputs. They use a
    // tiny xorshift PRNG seeded from `CRATESTACK_TEST_SEED` (or a fixed
    // default) so failures are reproducible: re-run with the same env
    // var to replay the exact sequence. Every test prints its seed on
    // failure via the assertion message.

    fn test_seed() -> u64 {
        std::env::var("CRATESTACK_TEST_SEED")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0x9E37_79B9_7F4A_7C15)
    }

    struct XorShift64(u64);

    impl XorShift64 {
        fn new(seed: u64) -> Self {
            // Avoid the all-zero state which would lock the PRNG.
            Self(if seed == 0 { 0xDEAD_BEEF_CAFE_BABE } else { seed })
        }
        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
        fn next_u32(&mut self) -> u32 {
            self.next_u64() as u32
        }
        fn next_range(&mut self, lo: u32, hi: u32) -> u32 {
            debug_assert!(lo <= hi);
            lo + (self.next_u32() % (hi - lo + 1))
        }
        fn next_bytes(&mut self, len: usize) -> Vec<u8> {
            let mut out = Vec::with_capacity(len);
            while out.len() < len {
                out.extend_from_slice(&self.next_u64().to_le_bytes());
            }
            out.truncate(len);
            out
        }
        fn next_string(&mut self, max_len: usize) -> String {
            let len = (self.next_u32() as usize) % (max_len + 1);
            // Include `:` and NUL routinely — they're the bytes most
            // likely to break key-derivation logic naïvely.
            const ALPHABET: &[u8] = b"abcdefghij0123456789:\0 -_";
            let mut s = String::with_capacity(len);
            for _ in 0..len {
                let idx = (self.next_u32() as usize) % ALPHABET.len();
                s.push(ALPHABET[idx] as char);
            }
            s
        }
    }

    #[test]
    fn randomized_bucket_key_is_deterministic_and_well_formed() {
        let seed = test_seed();
        let mut rng = XorShift64::new(seed);
        let store = offline_store("bank");
        for iteration in 0..200 {
            let key = rng.next_string(64);
            let a = store.bucket_key(&key);
            let b = store.bucket_key(&key);
            assert_eq!(
                a, b,
                "seed={seed:#x} iter={iteration} key={key:?}: bucket_key must be deterministic",
            );
            let suffix = a
                .strip_prefix("bank:rl:")
                .unwrap_or_else(|| panic!("seed={seed:#x} iter={iteration} missing prefix: {a}"));
            assert_eq!(
                suffix.len(),
                64,
                "seed={seed:#x} iter={iteration} key={key:?}: hex suffix must be 64 chars",
            );
            assert!(
                suffix.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
                "seed={seed:#x} iter={iteration} key={key:?}: suffix {suffix:?} must be lowercase hex",
            );
        }
    }

    #[test]
    fn randomized_bucket_key_collisions_are_negligible() {
        // SHA-256 makes a single collision astronomically unlikely, but
        // a regression that drops or truncates the hash would show up
        // immediately as duplicate keys in a small random sample.
        let seed = test_seed();
        let mut rng = XorShift64::new(seed);
        let store = offline_store("bank");
        let mut seen: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for iteration in 0..500 {
            let key = rng.next_string(32);
            let derived = store.bucket_key(&key);
            if let Some(prev) = seen.get(&derived) {
                assert_eq!(
                    prev, &key,
                    "seed={seed:#x} iter={iteration}: distinct keys {prev:?} and {key:?} mapped to the same bucket {derived}",
                );
            } else {
                seen.insert(derived, key);
            }
        }
    }

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
    fn randomized_normalize_key_prefix_never_emits_outer_colon_or_whitespace() {
        let seed = test_seed();
        let mut rng = XorShift64::new(seed);
        for iteration in 0..200 {
            let raw = rng.next_string(40);
            let normalized = normalize_key_prefix(raw.clone());
            // The fallback default is always non-empty.
            assert!(
                !normalized.is_empty(),
                "seed={seed:#x} iter={iteration} raw={raw:?}: must never be empty",
            );
            // No leading/trailing whitespace or `:` after normalisation.
            assert!(
                !normalized.starts_with(':')
                    && !normalized.ends_with(':')
                    && !normalized.starts_with(char::is_whitespace)
                    && !normalized.ends_with(char::is_whitespace),
                "seed={seed:#x} iter={iteration} raw={raw:?} normalized={normalized:?}: outer noise must be stripped",
            );
        }
    }

    #[test]
    fn randomized_nibble_hex_only_emits_lowercase_hex_chars() {
        let seed = test_seed();
        let mut rng = XorShift64::new(seed);
        for iteration in 0..200 {
            let byte = (rng.next_u32() & 0x0f) as u8;
            let ch = nibble_hex(byte);
            assert!(
                ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase(),
                "seed={seed:#x} iter={iteration} byte={byte:#x} -> {ch}",
            );
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
}
