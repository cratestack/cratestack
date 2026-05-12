# cratestack-client-store-sqlite

SQLite-backed `ClientStateStore` implementation for the CrateStack Rust client.

## Overview

`cratestack-client-store-sqlite` persists the client-side request journal in a local SQLite database. It is the right choice for single-device deployments — desktop apps, headless agents, or the Rust core of an offline-first mobile app.

The store implements the `ClientStateStore` trait from `cratestack-client-rust` and bundles SQLite via `rusqlite`'s `bundled` feature so no system library is required.

## Installation

```toml
[dependencies]
cratestack-client-store-sqlite = "0.2.2"
cratestack-client-rust = "0.2.2"
```

## Usage

```rust
use std::sync::Arc;
use cratestack_client_rust::{CborCodec, ClientConfig, ClientStateStore, CratestackClient};
use cratestack_client_store_sqlite::SqliteStateStore;

let store: Arc<dyn ClientStateStore> = Arc::new(SqliteStateStore::open("./client_state.db")?);

let base_url = url::Url::parse("https://api.example.com")?;
let client = CratestackClient::new(ClientConfig::new(base_url), CborCodec)
    .with_state_store(store);
```

The store creates parent directories on `open`, applies its schema migration on first use, and serialises access through an internal `Mutex`.

## Storage Schema

```sql
CREATE TABLE state_meta (
  singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
  schema_version INTEGER NOT NULL,
  state_version INTEGER NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE request_journal (
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  method TEXT NOT NULL,
  path TEXT NOT NULL,
  status_code INTEGER NOT NULL,
  content_type TEXT,
  recorded_at TEXT NOT NULL
);
```

## See Also

- `cratestack-client-rust` — client runtime and `ClientStateStore` trait
- `cratestack-client-store-redis` — Redis-backed alternative for multi-instance deployments
- [Client Runtime](https://cratestack.dev/architecture/client-runtime)

## License

MIT
