use std::sync::LazyLock;

use redis::Script;

pub(super) const CONSUME_LUA: &str = r#"
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

pub(super) static CONSUME_SCRIPT: LazyLock<Script> = LazyLock::new(|| Script::new(CONSUME_LUA));
