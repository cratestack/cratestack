# cratestack-client-store-sqlite

SQLite-backed state store for client-side persistence.

## Overview

`cratestack-client-store-sqlite` provides a SQLite implementation of `ClientStateStore` for persisting request journals on device, suitable for desktop and mobile applications.

## Installation

```toml
[dependencies]
cratestack-client-store-sqlite = "0.2"
```

## Usage

```rust
use cratestack_client_store_sqlite::SqliteStateStore;
use cratestack_client_rust::{CratestackClient, ClientConfig, CborCodec};
use std::sync::Arc;

let store = Arc::new(SqliteStateStore::new("./client_state.db")?);

let client = CratestackClient::new(
    ClientConfig::new("https://api.example.com"),
    CborCodec,
).with_state_store(store);

// Requests are journaled to SQLite
let result = client.post("/transfer", &input, &[]).await?;
```

## Features

- **Bundled SQLite**: Works out of the box with `rusqlite` bundled feature
- **Automatic Migration**: Tables created on first use
- **Thread-Safe**: Concurrent access via internal mutex

## Storage Schema

```sql
CREATE TABLE request_journal (
    id TEXT PRIMARY KEY,
    path TEXT NOT NULL,
    method TEXT NOT NULL,
    body BLOB,
    status_code INTEGER,
    created_at TEXT NOT NULL
);
```

## Use Cases

- **Desktop Applications**: Persist state for desktop clients
- **Offline-First Apps**: Queue mutations when offline
- **Single-Device State**: Local-only persistence without server dependency

## See Also

- `cratestack-client-store-redis` - Redis-backed store (multi-instance)
- `cratestack-client-rust` - Client runtime

## License

MIT