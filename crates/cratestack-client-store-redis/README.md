# cratestack-client-store-redis

Redis-backed `ClientStateStore` implementation for the CrateStack Rust client.

## Overview

`cratestack-client-store-redis` persists the client-side request journal in Redis so multiple client instances (or a process restart) can share state and replay pending mutations after recovery.

The store implements the `ClientStateStore` trait from `cratestack-client-rust`.

## Installation

```toml
[dependencies]
cratestack-client-store-redis = "0.2.2"
cratestack-client-rust = "0.2.2"
redis = "1"
```

## Usage

```rust
use std::sync::Arc;
use cratestack_client_rust::{CborCodec, ClientConfig, ClientStateStore, CratestackClient};
use cratestack_client_store_redis::RedisStateStore;

let store: Arc<dyn ClientStateStore> =
    Arc::new(RedisStateStore::open("redis://127.0.0.1:6379", "myapp:journals")?);

let base_url = url::Url::parse("https://api.example.com")?;
let client = CratestackClient::new(ClientConfig::new(base_url), CborCodec)
    .with_state_store(store);
```

To reuse an existing `redis::Client`:

```rust
let redis_client = redis::Client::open("redis://127.0.0.1:6379")?;
let store = RedisStateStore::from_client(redis_client, "myapp:journals");
```

`RedisStateStoreConfig` exposes a `key_prefix` field; the prefix is normalised (leading/trailing colons stripped) and falls back to `cratestack:client` when empty. The store writes a `<prefix>:meta` hash and a `<prefix>:request_journal` list.

## See Also

- `cratestack-client-rust` — client runtime and `ClientStateStore` trait
- `cratestack-client-store-sqlite` — SQLite-backed alternative for single-device deployments
- [Client Runtime](https://cratestack.dev/architecture/client-runtime)

## License

MIT
