# cratestack-client-store-redis

Redis-backed state store for request journaling.

## Overview

`cratestack-client-store-redis` provides a Redis implementation of `ClientStateStore` for persisting client-side request journals across instances in multi-instance deployments.

## Installation

```toml
[dependencies]
cratestack-client-store-redis = "0.2"
redis = "0.24"
```

## Usage

```rust
use cratestack_client_store_redis::RedisStateStore;
use cratestack_client_rust::{CratestackClient, ClientConfig, CborCodec};
use std::sync::Arc;

let redis = redis::Client::open("redis://127.0.0.1:6379")?;
let store = Arc::new(RedisStateStore::new(redis, "myapp:journals:")?);

let client = CratestackClient::new(
    ClientConfig::new("https://api.example.com"),
    CborCodec,
).with_state_store(store);

// Requests are journaled to Redis
let result = client.post("/transfer", &input, &[]).await?;
```

## Configuration

```rust
use cratestack_client_store_redis::RedisStateStoreConfig;

let config = RedisStateStoreConfig {
    key_prefix: "myapp:journals:".to_owned(),
    ttl_seconds: Some(86400), // 24 hours
};

let store = RedisStateStore::with_config(redis, config)?;
```

## Use Cases

- **Request Journaling**: Persist mutations for retry after crash
- **Multi-Instance State**: Share state across multiple client instances
- **Offline Queue**: Queue operations when offline (with sync on reconnect)

## See Also

- `cratestack-client-store-sqlite` - SQLite-backed store (single-device)
- `cratestack-client-rust` - Client runtime

## License

MIT