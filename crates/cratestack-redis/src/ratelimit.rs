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
use std::time::SystemTime;

use async_trait::async_trait;
use cratestack_axum::ratelimit::{RateLimitConfig, RateLimitDecision, RateLimitStore};
use cratestack_core::CoolError;
use redis::{Script, Value as RedisValue};

use crate::support::{
    next_string, next_u32_decimal, redis_error, system_time_to_ms, KeyNamespace,
};

const SCOPE: &str = "redis rate limit";

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
            key_prefix: KeyNamespace::new(key_prefix, "rl").prefix().to_owned(),
        }
    }
}

#[derive(Clone)]
pub struct RedisRateLimitStore {
    client: redis::Client,
    namespace: KeyNamespace,
}

impl RedisRateLimitStore {
    pub fn open(
        redis_url: impl redis::IntoConnectionInfo,
        key_prefix: impl Into<String>,
    ) -> Result<Self, CoolError> {
        let client = redis::Client::open(redis_url).map_err(|e| redis_error(SCOPE, e))?;
        Ok(Self::from_client(client, key_prefix))
    }

    pub fn from_client(client: redis::Client, key_prefix: impl Into<String>) -> Self {
        Self {
            client,
            namespace: KeyNamespace::new(key_prefix, "rl"),
        }
    }

    pub fn key_prefix(&self) -> &str {
        self.namespace.prefix()
    }

    pub fn bucket_key(&self, key: &str) -> String {
        self.namespace.hashed_key(&[key.as_bytes()])
    }

    async fn connection(&self) -> Result<redis::aio::MultiplexedConnection, CoolError> {
        self.client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| redis_error(SCOPE, e))
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
        let now_ms = system_time_to_ms(SCOPE, SystemTime::now())?;
        let bucket_key = self.bucket_key(key);

        // Lua's `tonumber` accepts standard decimal notation; we serialise
        // the float with `{}` so values like `0.001` round-trip through
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
            .map_err(|e| redis_error(SCOPE, e))?;

        parse_consume_outcome(value)
    }
}

fn parse_consume_outcome(value: RedisValue) -> Result<RateLimitDecision, CoolError> {
    let items = match value {
        RedisValue::Array(items) => items,
        other => {
            return Err(CoolError::Internal(format!(
                "{SCOPE}: expected array from consume script, got {other:?}"
            )));
        }
    };
    let mut iter = items.into_iter();
    let tag = next_string(SCOPE, &mut iter, "tag")?;
    match tag.as_str() {
        "allowed" => {
            let remaining = next_u32_decimal(SCOPE, &mut iter, "remaining")?;
            Ok(RateLimitDecision::Allowed { remaining })
        }
        "throttled" => {
            let retry_after_secs = next_u32_decimal(SCOPE, &mut iter, "retry_after_secs")?;
            Ok(RateLimitDecision::Throttled { retry_after_secs })
        }
        other => Err(CoolError::Internal(format!(
            "{SCOPE}: unexpected outcome tag: {other}"
        ))),
    }
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

    // ----- Randomized property tests -----
    //
    // Tiny xorshift PRNG seeded from `CRATESTACK_TEST_SEED` (or a fixed
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
            // Use the random byte path too, for variety.
            let _ = rng.next_bytes(8);
        }
    }
}
