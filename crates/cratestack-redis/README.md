# cratestack-redis

Redis-backed server-side infrastructure for CrateStack.

## Overview

`cratestack-redis` provides Redis implementations of CrateStack server-side interfaces. Currently it provides `RedisIdempotencyStore` - a Redis-backed implementation of the `IdempotencyStore` trait used by `IdempotencyLayer` for duplicate execution protection.

This is the Redis equivalent of the Postgres-backed `SqlxIdempotencyStore` in `cratestack-sqlx`.

## Installation

```toml
[dependencies]
cratestack-redis = "0.2"
redis = { version = "0.24", features = ["aio", "tokio-comp", "script"] }
```

## Usage

### Idempotency Store

Use with `IdempotencyLayer` for duplicate execution protection:

```rust
use cratestack_redis::RedisIdempotencyStore;
use cratestack_axum::idempotency::IdempotencyLayer;

let store = RedisIdempotencyStore::open(
    "redis://127.0.0.1:6379",
    "myapp:idem"
)?;

let layer = IdempotencyLayer::new(store);

let app = axum::Router::new()
    .route("/api", handler)
    .layer(layer);
```

### Configuration

```rust
use cratestack_redis::RedisIdempotencyStoreConfig;

let config = RedisIdempotencyStoreConfig::new("bank:au:idem");
// Or use from_client for connection pooling

let client = redis::Client::open("redis://cluster:6379")?;
let store = RedisIdempotencyStore::from_client(client, "bank");
```

## How It Works

### Key Structure

Each `(principal, key)` pair maps to a Redis hash:

```
<prefix>:idem:<sha256(principal || 0x00 || key)>
```

SHA-256 hashing keeps Redis keys bounded regardless of input size and avoids escaping concerns around `:` in user-supplied values.

### Atomicity

Three Lua scripts provide atomicity:

1. **RESERVE_LUA** - Atomically checks for existing entry, creates new reservation, or returns cached response
2. **COMPLETE_LUA** - Stores response with token validation
3. **RELEASE_LUA** - Drops pending reservation if status is `in_flight`

Redis handles `EVALSHA` plus `NOSCRIPT` fallback automatically via the `redis` crate.

### Eviction

Keys are evicted via `PEXPIREAT` based on the provided `expires_at`. When TTL passes:
- Redis drops the hash
- Next reservation starts fresh
- Late `complete/release` from previous reservation becomes silent no-op

## Use Cases

- **Multi-instance deployments**: Redis as durable store shared across replicas
- **Banking-grade idempotency**: Same protection as Postgres store, for Redis-preferring deployments
- **Existing infrastructure**: Leverage existing Redis clusters instead of adding Postgres

## Comparison with Postgres Store

| Feature | `SqlxIdempotencyStore` | `RedisIdempotencyStore` |
|---------|------------------------|-------------------------|
| Backend | Postgres table | Redis hash |
| Transactions | Participates in existing tx | Independent |
| Cluster support | Postgres replication | Redis Cluster/replication |
| Use case | Transactional audit coupling | Existing Redis infra |

## See Also

- [Idempotency Guide](https://cratestack.dev/guides/idempotency)
- `cratestack-axum::idempotency` - Idempotency Layer
- `cratestack-client-store-redis` - Client-side Redis state store (different use case)

## License

MIT