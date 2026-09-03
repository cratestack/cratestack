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
//! # The scope set outlives the buckets it admitted (cratestack#871 review)
//!
//! The first cut suffixed the scope key with a fixed-window epoch and gave
//! it `PEXPIRE window_ms` while buckets got `EXPIRE bucket_ttl`. With
//! `window < bucket_ttl` that bounded nothing — every rollover minted a
//! fresh key that re-admitted `max_distinct` more buckets while the
//! previous generation was still alive, so the steady state was
//! `max_distinct * ceil(bucket_ttl / window)`. Measured: 21 buckets for a
//! cap of 4 over five 1s windows, and ~184 320 per peer for a
//! non-refilling bucket under the defaults — past the in-memory cap on its
//! own.
//!
//! So there is no epoch. One key per scope, `PEXPIRE`d to
//! `cratestack_core::scope_ttl_secs` (at least the bucket TTL) on every
//! hit. Dropping the epoch also removes a clock-skew multiplier: replicas
//! disagreeing on `now_ms` used to land on different epoch keys and each
//! mint their own generation.
//!
//! # The window slides, per member (cratestack#871 round-2, item 3)
//!
//! The scope is a **`ZSET` scored by last use**, not a `SET`. A shared
//! deadline for the whole scope forced a choice between two defects:
//! refreshing it on every hit capped a token-rotating peer at its first
//! `max_distinct` credentials forever, and refreshing only on admission
//! left a transient `2 x max_distinct` (a bucket kept alive by traffic
//! could outlive the record that admitted it, freeing a slot for another
//! while it was still there).
//!
//! Per-member expiry removes the choice. `ZREMRANGEBYSCORE` trims slots
//! whose credential has gone quiet for `scope_ttl`; an actively-used
//! member's score is refreshed, so its slot is never freed while its
//! bucket is alive. The bound is then `max_distinct` live buckets per
//! scope with no transient overshoot. Cost is one extra O(log N) command
//! at N <= 128, in the same round-trip.
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
local scope_ttl_ms = tonumber(ARGV[6])
local member = ARGV[7]

-- KEYS[1] requested bucket, KEYS[2] scope set, KEYS[3] fallback bucket.
-- max_distinct < 0 means "no budget": KEYS[2] and KEYS[3] are then passed
-- as copies of KEYS[1] and are never touched.
local target = KEYS[1]
local charged = 'requested'
if max_distinct >= 0 then
  -- Slide the window: drop members whose slot aged out. Scores are
  -- LAST-USE timestamps, so this frees the slots of credentials the peer
  -- has stopped using without ever evicting one it is still using.
  redis.call('ZREMRANGEBYSCORE', KEYS[2], '-inf', now_ms - scope_ttl_ms)
  if redis.call('ZSCORE', KEYS[2], member) then
    -- Already admitted: keep its own bucket even if the scope is now
    -- saturated, so an attacker cannot displace callers that were under
    -- the cap first. ZADD refreshes the score rather than adding a member.
    redis.call('ZADD', KEYS[2], now_ms, member)
  elseif redis.call('ZCARD', KEYS[2]) < max_distinct then
    redis.call('ZADD', KEYS[2], now_ms, member)
  else
    target = KEYS[3]
    charged = 'fallback'
  end
  -- Unconditional, on every hit and not only on admission. The record must
  -- outlive every bucket it admitted (scope_ttl_ms >= the bucket EXPIRE
  -- below) or a fresh generation opens underneath a live one; and
  -- re-arming it every time repairs a key that lost its TTL because an
  -- earlier script aborted between ZADD and PEXPIRE.
  redis.call('PEXPIRE', KEYS[2], scope_ttl_ms)
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
