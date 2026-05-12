# cratestack-redis

Redis-backed server-side infrastructure for CrateStack.

## Overview

`cratestack-redis` provides Redis implementations of the server-side traits CrateStack defines. Today the crate ships a single store:

- `RedisIdempotencyStore` — implements the `IdempotencyStore` trait from `cratestack-axum::idempotency`, the Redis equivalent of `SqlxIdempotencyStore` for deployments that prefer Redis to a Postgres `idempotency_keys` table.

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

`RedisIdempotencyStoreConfig` exposes a single `key_prefix` field; the prefix is normalised (leading/trailing colons stripped) and falls back to a built-in default when empty. TTL is owned by the layer (via `IdempotencyLayer::new(..., ttl)`), not the store, and is applied per record through `PEXPIREAT`.

## How It Works

Each `(principal, key)` pair maps to a Redis hash keyed by `<prefix>:idem:<sha256(principal || 0x00 || key)>`. SHA-256 hashing keeps Redis keys bounded regardless of input size and avoids escaping concerns around `:` in user-supplied values.

Atomicity is provided by three Lua scripts:

- `RESERVE_LUA` — atomically checks for an existing entry, creates a new reservation, or returns the cached response.
- `COMPLETE_LUA` — stores the response with a token check so a stale completer cannot overwrite a fresh record.
- `RELEASE_LUA` — drops a pending reservation if its status is still `in_flight`.

Eviction uses `PEXPIREAT` based on the `expires_at` derived from the layer's TTL.

## See Also

- [Idempotency guide](https://cratestack.dev/guides/idempotency)
- `cratestack-axum` — `IdempotencyLayer`, `IdempotencyStore`, table DDL helper
- `cratestack-sqlx` — Postgres-backed `SqlxIdempotencyStore`
- `cratestack-client-store-redis` — Redis state store for the client runtime (unrelated trait)

## License

MIT
