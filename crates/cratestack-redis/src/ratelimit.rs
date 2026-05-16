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

mod config;
mod parse;
mod scripts;
mod store;
mod time;
mod trait_impl;
mod util;

#[cfg(test)]
mod tests_config;
#[cfg(test)]
mod tests_fixtures;
#[cfg(test)]
mod tests_helpers;
#[cfg(test)]
mod tests_parse;
#[cfg(test)]
mod tests_randomized_keys;
#[cfg(test)]
mod tests_randomized_parse;

pub use config::RedisRateLimitStoreConfig;
pub use store::RedisRateLimitStore;
