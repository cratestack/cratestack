# cratestack-redis

Redis-backed server-side infrastructure for CrateStack.

## Overview

`cratestack-redis` provides Redis implementations of the server-side traits CrateStack defines. The crate ships two stores:

- `RedisIdempotencyStore` — implements the `IdempotencyStore` trait from `cratestack-axum::idempotency`, the Redis equivalent of `SqlxIdempotencyStore` for deployments that prefer Redis to a Postgres `idempotency_keys` table.
- `RedisRateLimitStore` — implements the `RateLimitStore` trait from `cratestack-axum::ratelimit`, sharing token-bucket state across replicas so a multi-instance deployment enforces a single global rate limit per key.

## Installation

```toml
[dependencies]
cratestack-redis = "0.2.2"
cratestack-axum = "0.2.2"
redis = "1"
```

## Usage

```rust
use std::sync::Arc;
use std::time::Duration;
use cratestack_axum::idempotency::{IdempotencyLayer, IdempotencyStore};
use cratestack_redis::RedisIdempotencyStore;

let store: Arc<dyn IdempotencyStore> =
    Arc::new(RedisIdempotencyStore::open("redis://127.0.0.1:6379", "myapp")?);

let app = axum::Router::new()
    .nest("/api", router)
    .layer(IdempotencyLayer::new(store, Duration::from_secs(24 * 60 * 60)));
```

To reuse an existing `redis::Client` (typical for connection pooling and Cluster setups):

```rust
let client = redis::Client::open("redis://cluster:6379")?;
let store = RedisIdempotencyStore::from_client(client, "myapp");
```

### TLS (`rediss://`)

Managed/HA Redis often exposes only a TLS listener, sometimes behind a
private or internal CA. Enable the `tls-rustls` feature to connect over
`rediss://`:

```toml
[dependencies]
cratestack-redis = { version = "0.2.2", features = ["tls-rustls"] }
```

```rust
use redis::TlsCertificates;
use cratestack_redis::RedisIdempotencyStore;

// System/webpki trust store (covers most managed Redis providers):
let store = RedisIdempotencyStore::open_with_tls(
    "rediss://redis.example.internal:6380",
    "myapp",
    TlsCertificates { client_tls: None, root_cert: None },
)?;

// Private/internal CA — pass a PEM-encoded bundle instead:
let root_cert = std::fs::read("internal-ca.pem")?;
let store = RedisIdempotencyStore::open_with_tls(
    "rediss://redis.example.internal:6380",
    "myapp",
    TlsCertificates { client_tls: None, root_cert: Some(root_cert) },
)?;
```

`RedisRateLimitStore` has the same `open_with_tls` constructor. Both stores
are async-only (`tokio-comp`), so `tls-rustls` forwards to the upstream
`redis` crate's `tokio-rustls-comp` feature — the one that actually wires
up the async TLS stream, not just URL parsing.

`RedisIdempotencyStoreConfig` exposes a single `key_prefix` field; the prefix is normalised (leading/trailing colons stripped) and falls back to a built-in default when empty. TTL is owned by the layer (via `IdempotencyLayer::new(..., ttl)`), not the store, and is applied per record through `PEXPIREAT`.

### Rate limit store

```rust
use std::sync::Arc;
use cratestack_axum::ratelimit::{RateLimitConfig, RateLimitLayer, RateLimitStore};
use cratestack_redis::RedisRateLimitStore;

let store: Arc<dyn RateLimitStore> =
    Arc::new(RedisRateLimitStore::open("redis://127.0.0.1:6379", "myapp")?);

let app = axum::Router::new()
    .nest("/api", router)
    .layer(RateLimitLayer::new(store, RateLimitConfig::new(100, 10.0)));
```

`RedisRateLimitStoreConfig` mirrors the idempotency config — a single normalised `key_prefix` field. Each bucket is stored under `<prefix>:rl:<sha256(key)>` and refreshes its TTL on every `consume`, so idle buckets evict themselves and memory stays bounded.

## How It Works

### Idempotency

Each `(principal, key)` pair maps to a Redis hash keyed by `<prefix>:idem:<sha256(principal || 0x00 || key)>`. SHA-256 hashing keeps Redis keys bounded regardless of input size and avoids escaping concerns around `:` in user-supplied values.

Atomicity is provided by three Lua scripts:

- `RESERVE_LUA` — atomically checks for an existing entry, creates a new reservation, or returns the cached response.
- `COMPLETE_LUA` — stores the response with a token check so a stale completer cannot overwrite a fresh record.
- `RELEASE_LUA` — drops a pending reservation if its status is still `in_flight`.

Eviction uses `PEXPIREAT` based on the `expires_at` derived from the layer's TTL.

### Rate limiting

Each rate-limit key maps to a Redis hash at `<prefix>:rl:<sha256(key)>` carrying two fields: `tokens` (current bucket fill) and `last_refill_ms` (the wall-clock timestamp of the most recent refill). A single Lua script does the entire read-refill-decrement-write cycle in one round-trip, so concurrent replicas can never grant more than one token's worth of overshoot.

Eviction uses a relative `EXPIRE` derived from the time required to refill a full bucket (clamped to 24h), refreshed on every `consume`. This keeps memory bounded even for tenant-scoped keyspaces with churn.

## See Also

- [Idempotency guide](https://cratestack.dev/guides/idempotency)
- `cratestack-axum` — `IdempotencyLayer`, `IdempotencyStore`, `RateLimitLayer`, `RateLimitStore`, table DDL helper
- `cratestack-sqlx` — Postgres-backed `SqlxIdempotencyStore`
- `cratestack-client-store-redis` — Redis state store for the client runtime (unrelated trait)

## License

MIT
