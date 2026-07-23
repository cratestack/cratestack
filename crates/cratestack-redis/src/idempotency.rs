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
mod tests_time;
#[cfg(all(test, feature = "tls-rustls"))]
mod tests_tls;

pub use config::RedisIdempotencyStoreConfig;
pub use store::RedisIdempotencyStore;
