//! Redis-backed [`IdempotencyStore`].
//!
//! Each `(principal, key)` pair maps to a single Redis hash keyed by
//! `<prefix>:idem:<sha256(principal || 0x00 || key)>`. Hashing both sides
//! keeps the Redis key bounded regardless of how long the principal
//! fingerprint or idempotency key gets, and avoids any escaping concerns
//! around `:` in user-supplied values.
//!
//! Atomicity comes from three small Lua scripts. The `redis` crate's
//! `Script::invoke_async` handles `EVALSHA` plus the `NOSCRIPT` fallback
//! automatically, so we don't manage SHA1s by hand.
//!
//! Eviction is driven by `PEXPIREAT` rather than an "expired" branch in
//! the scripts: Redis drops the hash when the TTL passes, the next
//! reservation observes a missing key and starts fresh, and any late
//! `complete`/`release` from the previous reservation finds a rotated
//! token and becomes a silent no-op — exactly the trait contract.

use std::sync::LazyLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use cratestack_axum::idempotency::{IdempotencyRecord, IdempotencyStore, ReservationOutcome};
use cratestack_core::CoolError;
use redis::{Script, Value as RedisValue};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const RESERVE_LUA: &str = r#"
local existing = redis.call('HMGET', KEYS[1], 'request_hash', 'status')
local rh = existing[1]
local st = existing[2]
if not rh then
  redis.call('HSET', KEYS[1],
    'request_hash', ARGV[1],
    'status', 'in_flight',
    'token', ARGV[2],
    'created_at', ARGV[4],
    'expires_at', ARGV[3],
    'principal', ARGV[5],
    'key', ARGV[6])
  redis.call('PEXPIREAT', KEYS[1], ARGV[3])
  return {'reserved', ARGV[2]}
end
if rh ~= ARGV[1] then return {'conflict'} end
if st == 'in_flight' then return {'in_flight'} end
if st == 'completed' then
  local r = redis.call('HMGET', KEYS[1],
    'response_status', 'response_headers', 'response_body',
    'created_at', 'expires_at')
  return {'replay', rh, r[1], r[2], r[3], r[4], r[5]}
end
return {'unknown'}
"#;

const COMPLETE_LUA: &str = r#"
local cur = redis.call('HGET', KEYS[1], 'token')
if not cur or cur ~= ARGV[1] then return 'token_mismatch' end
redis.call('HSET', KEYS[1],
  'status', 'completed',
  'response_status', ARGV[2],
  'response_headers', ARGV[3],
  'response_body', ARGV[4])
local exp = redis.call('HGET', KEYS[1], 'expires_at')
if exp then redis.call('PEXPIREAT', KEYS[1], exp) end
return 'ok'
"#;

// The `status == 'in_flight'` guard matches the SQL version's
// `AND response_body IS NULL` clause: release is meant to drop a
// pending reservation when the handler bailed out, not to wipe an
// already-captured response. Without this guard a caller that
// mistakenly invoked both `complete` and `release` would lose the
// cached response — the middleware never does this, but it's a cheap
// guarantee for anyone using the store trait directly.
const RELEASE_LUA: &str = r#"
local r = redis.call('HMGET', KEYS[1], 'token', 'status')
if r[1] and r[1] == ARGV[1] and r[2] == 'in_flight' then
  redis.call('DEL', KEYS[1])
end
return 'ok'
"#;

static RESERVE_SCRIPT: LazyLock<Script> = LazyLock::new(|| Script::new(RESERVE_LUA));
static COMPLETE_SCRIPT: LazyLock<Script> = LazyLock::new(|| Script::new(COMPLETE_LUA));
static RELEASE_SCRIPT: LazyLock<Script> = LazyLock::new(|| Script::new(RELEASE_LUA));

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedisIdempotencyStoreConfig {
    pub key_prefix: String,
}

impl RedisIdempotencyStoreConfig {
    pub fn new(key_prefix: impl Into<String>) -> Self {
        Self {
            key_prefix: normalize_key_prefix(key_prefix.into()),
        }
    }
}

#[derive(Clone)]
pub struct RedisIdempotencyStore {
    client: redis::Client,
    config: RedisIdempotencyStoreConfig,
}

impl RedisIdempotencyStore {
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
            config: RedisIdempotencyStoreConfig::new(key_prefix),
        }
    }

    pub fn key_prefix(&self) -> &str {
        &self.config.key_prefix
    }

    pub fn hash_key(&self, principal: &str, key: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(principal.as_bytes());
        hasher.update([0u8]);
        hasher.update(key.as_bytes());
        let digest = hasher.finalize();
        let mut out = String::with_capacity(self.config.key_prefix.len() + 6 + 64);
        out.push_str(&self.config.key_prefix);
        out.push_str(":idem:");
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
impl IdempotencyStore for RedisIdempotencyStore {
    async fn reserve_or_fetch(
        &self,
        principal: &str,
        key: &str,
        request_hash: [u8; 32],
        expires_at: SystemTime,
    ) -> Result<ReservationOutcome, CoolError> {
        let mut conn = self.connection().await?;
        let hashkey = self.hash_key(principal, key);
        let new_token = Uuid::new_v4();
        let expires_ms = system_time_to_ms(expires_at)?;
        let created_ms = system_time_to_ms(SystemTime::now())?;

        let value: RedisValue = RESERVE_SCRIPT
            .key(hashkey)
            .arg(request_hash.as_slice())
            .arg(new_token.as_bytes().as_slice())
            .arg(expires_ms.to_string())
            .arg(created_ms.to_string())
            .arg(principal.as_bytes())
            .arg(key.as_bytes())
            .invoke_async(&mut conn)
            .await
            .map_err(redis_error)?;

        parse_reserve_outcome(value, principal, key)
    }

    async fn complete(
        &self,
        principal: &str,
        key: &str,
        token: Uuid,
        status: u16,
        headers: &[u8],
        body: &[u8],
    ) -> Result<(), CoolError> {
        let mut conn = self.connection().await?;
        let hashkey = self.hash_key(principal, key);
        // The Lua script reads `expires_at` straight off the hash so we
        // don't need a separate round-trip and the PEXPIREAT stays
        // atomic with the response write. A mismatched token short-
        // circuits before either touches Redis.
        let outcome: RedisValue = COMPLETE_SCRIPT
            .key(hashkey)
            .arg(token.as_bytes().as_slice())
            .arg(u32::from(status).to_string())
            .arg(headers)
            .arg(body)
            .invoke_async(&mut conn)
            .await
            .map_err(redis_error)?;

        match value_as_string(&outcome).as_deref() {
            // `token_mismatch` is the documented silent no-op: a stale
            // handler whose reservation got reclaimed must not surface
            // an error, otherwise the inner service sees a spurious
            // failure for a successful request.
            Some("ok") | Some("token_mismatch") => Ok(()),
            other => Err(CoolError::Internal(format!(
                "redis idempotency: unexpected complete result: {other:?}"
            ))),
        }
    }

    async fn release(&self, principal: &str, key: &str, token: Uuid) -> Result<(), CoolError> {
        let mut conn = self.connection().await?;
        let hashkey = self.hash_key(principal, key);
        let _: RedisValue = RELEASE_SCRIPT
            .key(hashkey)
            .arg(token.as_bytes().as_slice())
            .invoke_async(&mut conn)
            .await
            .map_err(redis_error)?;
        Ok(())
    }
}

fn parse_reserve_outcome(
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

fn next_string<I: Iterator<Item = RedisValue>>(iter: &mut I, field: &str) -> Result<String, CoolError> {
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

fn next_bytes<I: Iterator<Item = RedisValue>>(iter: &mut I, field: &str) -> Result<Vec<u8>, CoolError> {
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

fn next_i64_decimal<I: Iterator<Item = RedisValue>>(iter: &mut I, field: &str) -> Result<i64, CoolError> {
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

fn next_u16_decimal<I: Iterator<Item = RedisValue>>(iter: &mut I, field: &str) -> Result<u16, CoolError> {
    let n = next_i64_decimal(iter, field)?;
    u16::try_from(n).map_err(|_| {
        CoolError::Internal(format!(
            "redis idempotency: {field} out of u16 range: {n}"
        ))
    })
}

fn value_as_string(value: &RedisValue) -> Option<String> {
    match value {
        RedisValue::SimpleString(s) => Some(s.clone()),
        RedisValue::BulkString(b) => std::str::from_utf8(b).ok().map(str::to_owned),
        RedisValue::Okay => Some("OK".to_owned()),
        _ => None,
    }
}

fn system_time_to_ms(time: SystemTime) -> Result<i64, CoolError> {
    let dur = time.duration_since(UNIX_EPOCH).map_err(|err| {
        CoolError::Internal(format!(
            "redis idempotency: timestamp before unix epoch: {err}"
        ))
    })?;
    i64::try_from(dur.as_millis()).map_err(|_| {
        CoolError::Internal("redis idempotency: timestamp out of i64 ms range".to_owned())
    })
}

fn system_time_from_ms(ms: i64) -> SystemTime {
    if ms >= 0 {
        UNIX_EPOCH + Duration::from_millis(ms as u64)
    } else {
        UNIX_EPOCH - Duration::from_millis(ms.unsigned_abs())
    }
}

fn normalize_key_prefix(key_prefix: String) -> String {
    // Trim in the order outer-whitespace → outer-colons → outer-whitespace
    // so inputs like `" : : "` (any mix of leading/trailing whitespace and
    // colon delimiters) reduce to empty and fall back to the default.
    // A previous order of `trim_matches(':').trim()` left whitespace-
    // wrapped colon noise in the prefix.
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
    CoolError::Internal(format!("redis idempotency: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ----- Config / key layout -----

    #[test]
    fn config_trims_outer_colons_and_whitespace() {
        assert_eq!(
            RedisIdempotencyStoreConfig::new(":bank:idem:").key_prefix,
            "bank:idem",
        );
        assert_eq!(
            RedisIdempotencyStoreConfig::new("  bank  ").key_prefix,
            "bank",
        );
    }

    #[test]
    fn config_falls_back_to_default_namespace_when_empty() {
        for input in ["", "::", ":::", "   ", " : : "] {
            assert_eq!(
                RedisIdempotencyStoreConfig::new(input).key_prefix,
                "cratestack",
                "input {input:?} should fall back to default namespace",
            );
        }
    }

    #[test]
    fn config_preserves_inner_colons() {
        // Inner `:` characters are deliberately allowed — Redis ACL
        // hierarchies use them, and stripping them would collapse
        // distinct namespaces like `bank:au:idem` and `bank:nz:idem`.
        assert_eq!(
            RedisIdempotencyStoreConfig::new("bank:au:idem").key_prefix,
            "bank:au:idem",
        );
    }

    fn offline_store(prefix: &str) -> RedisIdempotencyStore {
        let client = redis::Client::open("redis://127.0.0.1/").expect("static URL must parse offline");
        RedisIdempotencyStore::from_client(client, prefix)
    }

    #[test]
    fn hash_key_uses_configured_prefix_and_idem_namespace() {
        let store = offline_store("bank");
        let key = store.hash_key("p", "k");
        let suffix = key
            .strip_prefix("bank:idem:")
            .expect("hash_key must use `<prefix>:idem:` as its namespace");
        assert_eq!(suffix.len(), 64);
        assert!(suffix.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn hash_key_is_deterministic() {
        let store = offline_store("bank");
        assert_eq!(store.hash_key("alice", "txn-1"), store.hash_key("alice", "txn-1"));
    }

    #[test]
    fn hash_key_disambiguates_principal_key_boundary() {
        // Naively concatenating `principal || key` would let ("ab","c")
        // and ("a","bc") collide on the same Redis hash, leaking one
        // tenant's response to another. The `0x00` separator (and the
        // SHA-256 wrap) makes that impossible.
        let store = offline_store("bank");
        assert_ne!(store.hash_key("ab", "c"), store.hash_key("a", "bc"));
        assert_ne!(store.hash_key("", "abc"), store.hash_key("abc", ""));
    }

    #[test]
    fn hash_key_isolates_different_prefixes() {
        // Two stores with different prefixes must produce different
        // Redis keys for the same `(principal, key)` — otherwise a
        // staging deployment could overwrite production rows.
        let a = offline_store("staging");
        let b = offline_store("prod");
        assert_ne!(a.hash_key("p", "k"), b.hash_key("p", "k"));
    }

    #[test]
    fn hash_key_handles_pathological_inputs() {
        let store = offline_store("bank");
        // Long principal + key — the SHA-256 wrap keeps the Redis key
        // bounded at exactly 64 hex chars regardless of input size.
        let long_principal = "x".repeat(10_000);
        let long_key = "y".repeat(10_000);
        let result = store.hash_key(&long_principal, &long_key);
        assert!(result.starts_with("bank:idem:"));
        assert_eq!(result.len(), "bank:idem:".len() + 64);
        // Inputs containing `:`, NUL bytes, and other delimiter-like
        // characters must round-trip without breaking the key
        // structure (the SHA-256 makes this trivially true, but a
        // future refactor that drops the hash needs to keep working).
        let weird = store.hash_key("p:rincipal\0", "k\0e:y");
        assert!(weird.starts_with("bank:idem:"));
        assert_eq!(weird.len(), "bank:idem:".len() + 64);
    }

    // ----- Time conversion -----

    #[test]
    fn system_time_ms_roundtrip_near_now() {
        let now = SystemTime::now();
        let ms = system_time_to_ms(now).expect("to ms");
        let back = system_time_from_ms(ms);
        let drift = now
            .duration_since(back)
            .or_else(|err| Ok::<_, std::time::SystemTimeError>(err.duration()))
            .unwrap();
        assert!(drift < Duration::from_millis(2), "roundtrip drift: {drift:?}");
    }

    #[test]
    fn system_time_from_ms_handles_negative_input() {
        // Negative `expires_at` shouldn't panic — Redis stores
        // arbitrary i64 values, and a buggy upstream could surface
        // one. We document the conversion as best-effort here.
        let result = system_time_from_ms(-1_000);
        assert!(result < UNIX_EPOCH);
    }

    #[test]
    fn system_time_to_ms_rejects_pre_epoch_inputs() {
        let before = UNIX_EPOCH - Duration::from_secs(1);
        let err = system_time_to_ms(before).expect_err("pre-epoch must error");
        assert!(matches!(err, CoolError::Internal(_)));
    }

    // ----- Hex / value helpers -----

    #[test]
    fn nibble_hex_covers_all_valid_nibbles() {
        let table: Vec<(u8, char)> = (0u8..=15)
            .map(|n| {
                (
                    n,
                    match n {
                        0 => '0',
                        1 => '1',
                        2 => '2',
                        3 => '3',
                        4 => '4',
                        5 => '5',
                        6 => '6',
                        7 => '7',
                        8 => '8',
                        9 => '9',
                        10 => 'a',
                        11 => 'b',
                        12 => 'c',
                        13 => 'd',
                        14 => 'e',
                        15 => 'f',
                        _ => unreachable!(),
                    },
                )
            })
            .collect();
        for (n, expected) in table {
            assert_eq!(nibble_hex(n), expected, "nibble {n}");
        }
    }

    #[test]
    fn value_as_string_extracts_text_variants() {
        assert_eq!(
            value_as_string(&RedisValue::SimpleString("ok".into())).as_deref(),
            Some("ok"),
        );
        assert_eq!(
            value_as_string(&RedisValue::BulkString(b"hello".to_vec())).as_deref(),
            Some("hello"),
        );
        assert_eq!(value_as_string(&RedisValue::Okay).as_deref(), Some("OK"));
        // Non-textual variants return None so the caller has to handle
        // them explicitly rather than silently coercing.
        assert!(value_as_string(&RedisValue::Int(1)).is_none());
        assert!(value_as_string(&RedisValue::Nil).is_none());
    }

    #[test]
    fn value_as_string_rejects_invalid_utf8_bulk_strings() {
        // Lone surrogate-style byte sequence isn't valid UTF-8. The
        // helper must report None rather than silently producing a
        // lossy string — otherwise we could mis-classify a `complete`
        // result.
        let bad = RedisValue::BulkString(vec![0xff, 0xfe, 0xfd]);
        assert!(value_as_string(&bad).is_none());
    }

    // ----- Reserve outcome parser -----

    fn bulk(s: &str) -> RedisValue {
        RedisValue::BulkString(s.as_bytes().to_vec())
    }
    fn raw_bulk(b: impl AsRef<[u8]>) -> RedisValue {
        RedisValue::BulkString(b.as_ref().to_vec())
    }

    #[test]
    fn parse_reserved_extracts_token_bytes() {
        let token = uuid::Uuid::new_v4();
        let value = RedisValue::Array(vec![bulk("reserved"), raw_bulk(token.as_bytes())]);
        let outcome =
            parse_reserve_outcome(value, "p", "k").expect("parse should succeed");
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
        let err = parse_reserve_outcome(bulk("reserved"), "p", "k")
            .expect_err("non-array root must error");
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

    // ----- Scalar helper edge cases -----

    #[test]
    fn next_i64_decimal_accepts_redis_int_directly() {
        // Redis returns Lua numbers as `Value::Int` rather than as a
        // decimal string, so the helper must accept both shapes.
        let mut iter = vec![RedisValue::Int(42)].into_iter();
        assert_eq!(next_i64_decimal(&mut iter, "x").unwrap(), 42);
    }

    #[test]
    fn next_i64_decimal_accepts_simple_string() {
        let mut iter = vec![RedisValue::SimpleString("123".into())].into_iter();
        assert_eq!(next_i64_decimal(&mut iter, "x").unwrap(), 123);
    }

    #[test]
    fn next_i64_decimal_rejects_garbage_bytes() {
        let mut iter = vec![raw_bulk(b"not-a-number")].into_iter();
        assert!(next_i64_decimal(&mut iter, "x").is_err());
    }

    #[test]
    fn next_u16_decimal_rejects_negative() {
        let mut iter = vec![bulk("-1")].into_iter();
        assert!(next_u16_decimal(&mut iter, "x").is_err());
    }

    #[test]
    fn next_string_rejects_invalid_utf8() {
        let mut iter = vec![RedisValue::BulkString(vec![0xff, 0xfe])].into_iter();
        assert!(next_string(&mut iter, "x").is_err());
    }

    #[test]
    fn next_bytes_treats_nil_as_empty() {
        // Redis returns Nil for unset hash fields. The replay parser
        // calls `next_bytes` on `response_headers`, which is allowed to
        // be empty — surfacing Nil as an error would break the empty-
        // headers case the integration tests rely on.
        let mut iter = vec![RedisValue::Nil].into_iter();
        assert_eq!(next_bytes(&mut iter, "x").unwrap(), Vec::<u8>::new());
    }
}
