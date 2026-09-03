//! The one Lua script both `consume` and `consume_bounded` run.
//!
//! # Why the cardinality budget is inside the script (cratestack#871)
//!
//! "Has this scope already minted this bucket / may it mint another /
//! charge the fallback instead" has to be atomic with the token
//! consumption. Done as a separate `SISMEMBER`+`SADD` round-trip, N
//! concurrent requests each read `SCARD < max` and each mint a bucket —
//! which is the amplification the budget exists to close, reintroduced as
//! a race. Folding it into the existing script also costs **zero extra
//! round-trips**: the same `EVALSHA` does both.
//!
//! # Redis Cluster
//!
//! Three keys in one script that are NOT hash-tagged to a common slot
//! means Cluster rejects the call with `CROSSSLOT`. That is deliberate and
//! documented rather than papered over with a shared hash tag: forcing a
//! peer's scope set, its buckets and its fallback into one slot would
//! concentrate an attacker's traffic on one node, and a `CROSSSLOT` is a
//! logical-class error, so it is refused loudly under every
//! `StoreErrorPolicy` instead of silently disabling the limiter. Cluster
//! is not a supported deployment for this store today.
//!
//! # The TTL is passed in, not computed here
//!
//! `ttl_sec` used to be `ceil(burst / refill) + 60`, clamped, computed in
//! Lua. cratestack#871 needs the identical horizon for the in-memory
//! store's eviction sweep, so the formula moved to
//! `cratestack_core::bucket_ttl_secs` and arrives as `ARGV[4]`. One
//! definition, no drift between the two backends.

use std::sync::LazyLock;

use redis::Script;

pub(super) const CONSUME_LUA: &str = r#"
local now_ms = tonumber(ARGV[1])
local burst = tonumber(ARGV[2])
local refill_per_second = tonumber(ARGV[3])
local ttl_sec = tonumber(ARGV[4])
local max_distinct = tonumber(ARGV[5])
local window_ms = tonumber(ARGV[6])
local member = ARGV[7]

-- KEYS[1] requested bucket, KEYS[2] scope set, KEYS[3] fallback bucket.
-- max_distinct < 0 means "no budget": KEYS[2] and KEYS[3] are then passed
-- as copies of KEYS[1] and are never touched.
local target = KEYS[1]
local charged = 'requested'
if max_distinct >= 0 then
  local card = redis.call('SCARD', KEYS[2])
  if redis.call('SISMEMBER', KEYS[2], member) == 1 then
    -- Already admitted this window: keep its own bucket even if the
    -- scope is now saturated, so an attacker cannot displace callers
    -- that were under the cap first.
  elseif card < max_distinct then
    redis.call('SADD', KEYS[2], member)
    if card == 0 then
      redis.call('PEXPIRE', KEYS[2], window_ms)
    end
  else
    target = KEYS[3]
    charged = 'fallback'
  end
end

local existing = redis.call('HMGET', target, 'tokens', 'last_refill_ms')
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

if tokens >= 1.0 then
  tokens = tokens - 1.0
  redis.call('HSET', target, 'tokens', tostring(tokens), 'last_refill_ms', tostring(now_ms))
  redis.call('EXPIRE', target, ttl_sec)
  local remaining = math.floor(tokens)
  if remaining < 0 then remaining = 0 end
  return {'allowed', tostring(remaining), charged}
else
  redis.call('HSET', target, 'tokens', tostring(tokens), 'last_refill_ms', tostring(now_ms))
  redis.call('EXPIRE', target, ttl_sec)
  local need = 1.0 - tokens
  local retry
  if refill_per_second > 0 then
    retry = math.ceil(need / refill_per_second)
  else
    retry = 86400
  end
  if retry < 1 then retry = 1 end
  return {'throttled', tostring(retry), charged}
end
"#;

pub(super) static CONSUME_SCRIPT: LazyLock<Script> = LazyLock::new(|| Script::new(CONSUME_LUA));
