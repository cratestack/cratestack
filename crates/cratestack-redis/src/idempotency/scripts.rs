use std::sync::LazyLock;

use redis::Script;

pub(super) const RESERVE_LUA: &str = r#"
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

pub(super) const COMPLETE_LUA: &str = r#"
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
pub(super) const RELEASE_LUA: &str = r#"
local r = redis.call('HMGET', KEYS[1], 'token', 'status')
if r[1] and r[1] == ARGV[1] and r[2] == 'in_flight' then
  redis.call('DEL', KEYS[1])
end
return 'ok'
"#;

pub(super) static RESERVE_SCRIPT: LazyLock<Script> = LazyLock::new(|| Script::new(RESERVE_LUA));
pub(super) static COMPLETE_SCRIPT: LazyLock<Script> = LazyLock::new(|| Script::new(COMPLETE_LUA));
pub(super) static RELEASE_SCRIPT: LazyLock<Script> = LazyLock::new(|| Script::new(RELEASE_LUA));
