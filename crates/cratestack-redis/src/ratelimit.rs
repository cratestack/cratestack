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
//!
//! ## Bucket cardinality is bounded too (cratestack#871)
//!
//! Per-bucket `EXPIRE` bounds how long a bucket lives, but not how many a
//! caller can create: `RateLimitLayer` runs before authentication, so a
//! caller rotating an unverified `Authorization` header used to mint one
//! `:rl:` key per request. [`RateLimitStore::consume_bounded`] closes
//! that. The same Lua script — no extra round-trip — additionally takes a
//! scope set at `<prefix>:rls:<sha256(scope)>` and a fallback bucket key:
//! on first sight of a bucket under a scope it `SADD`s it if
//! `SCARD < max_distinct`, and otherwise charges the *fallback* bucket
//! instead. Steady-state keyspace becomes O(scopes × max_distinct).
//!
//! The scope set is `PEXPIRE`d to `cratestack_core::scope_ttl_secs` — at
//! least the bucket TTL — on **every** admission, so the record always
//! outlives the buckets it admitted. An earlier cut suffixed the key with
//! a fixed-window epoch and expired it after the window: with
//! `window < bucket_ttl` that let each rollover re-admit `max_distinct`
//! more buckets on top of a still-live generation (measured: 21 buckets
//! for a cap of 4 over five 1s windows). Re-`PEXPIRE`ing on every
//! admission also repairs a set left without a TTL by a script that
//! aborted between `SADD` and `PEXPIRE` — Lua here is atomic but not
//! transactional, so a mid-script `OOM` can stop it partway.
//!
//! **Redis Cluster is not supported by this store**, and cratestack#871
//! makes that louder rather than papering over it: three un-hash-tagged
//! keys in one script means `CROSSSLOT`, which classifies as
//! logical-class and is therefore refused under every `StoreErrorPolicy`.
//! Forcing the three into one slot with a shared hash tag would
//! concentrate an attacker's traffic on a single node, so it is
//! deliberately not done. See `scripts.rs` for the full argument.

mod config;
mod parse;
mod retry;
mod scripts;
mod store;
mod time;
mod trait_impl;
mod util;

#[cfg(test)]
mod tests_config;
#[cfg(test)]
mod tests_error_class;
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
#[cfg(test)]
mod tests_retry;
#[cfg(test)]
mod tests_store;
#[cfg(all(test, feature = "tls-rustls"))]
mod tests_tls;

pub use config::RedisRateLimitStoreConfig;
pub use store::RedisRateLimitStore;
