# cratestack-rusqlite

SQLite backend for on-device, offline-first applications.

## Overview

`cratestack-rusqlite` provides a synchronous SQLite backend for CrateStack, designed for mobile and embedded applications that run the same `.cstack` schema on-device. It uses a sync API suitable for FFI bridges to Flutter and other UI toolkits.

## Installation

```toml
[dependencies]
cratestack-rusqlite = "0.2"
```

For mobile builds without the server stack:

```toml
[dependencies]
cratestack-rusqlite = "0.2"
cratestack-sql = "0.2"
```

## Note

Auth policies (`@@allow`, `@@deny`) are **not enforced** at the SQL layer on-device. The device is single-user; authorization is the app's concern.

## Usage

### Schema

Use `provider = "sqlite"`:

```cstack
datasource db {
  provider = "sqlite"
  url = env("DATABASE_URL")
}

model Note {
  id String @id
  title String
  body String
  pinned Boolean
  createdAt DateTime
}
```

### Runtime

```rust
use cratestack::include_schema;
use cratestack::{RusqliteRuntime, rusqlite_backend::ddl::create_table_sql};
use cratestack_rusqlite::ModelDelegate;

include_schema!("schema.cstack");

fn open_store() -> Result<RusqliteRuntime, Box<dyn std::error::Error>> {
    let runtime = RusqliteRuntime::open("app.db")?;

    // Bootstrap tables
    runtime.with_connection(|conn| {
        conn.execute_batch(&create_table_sql(&cratestack_schema::NOTE_MODEL))?;
        Ok(())
    })?;

    Ok(runtime)
}
```

### CRUD Operations

```rust
fn example(runtime: &RusqliteRuntime) -> Result<(), Box<dyn std::error::Error>> {
    let notes = ModelDelegate::new(runtime, &cratestack_schema::NOTE_MODEL);

    // Create (sync API, no tokio)
    let created = notes.create(CreateNoteInput {
        id: uuid::Uuid::new_v4(),
        title: "First note".into(),
        body: "Hello.".into(),
        pinned: true,
        createdAt: chrono::Utc::now(),
    }).run()?;

    // Find with filters
    let pinned = notes
        .find_many()
        .where_(note::pinned().is_true())
        .order_by(note::createdAt().desc())
        .limit(20)
        .run()?;

    // Update
    notes.update(created.id, UpdateNoteInput {
        title: Some("Updated".into()),
        ..Default::default()
    }).run()?;

    // Delete
    notes.delete(created.id).run()?;

    Ok(())
}
```

## Storage Classes

SQLite columns use BLOB affinity to preserve precision:

| Type | Stored As |
|------|-----------|
| String, Cuid | TEXT |
| Int | INTEGER |
| Float | REAL |
| Bool | INTEGER (0/1) |
| Bytes | BLOB |
| Uuid | TEXT (canonical hyphenated) |
| DateTime | TEXT (RFC 3339 UTC) |
| Json | TEXT (compact serde JSON) |
| Decimal | TEXT (canonical string, exact precision) |

## Soft Delete

`@@soft_delete` works on device - DELETE becomes UPDATE, find queries filter soft-deleted rows.

## FFI Bridge

```rust
use cratestack_rusqlite::ffi::{OperationRequest, OperationResponse, json_request_from, json_response_into};

fn ffi_call(runtime: &RusqliteRuntime, bytes: &[u8]) -> Vec<u8> {
    let request = match json_request_from(bytes) {
        Ok(req) => req,
        Err(err) => return json_response_into(&OperationResponse::err("bad_request", err.to_string())),
    };
    json_response_into(&dispatch(runtime, request))
}
```

## See Also

- [Offline-First with SQLite](https://cratestack.dev/guides/offline-first-sqlite)
- `cratestack-sqlx` - Postgres backend (server)
- `cratestack-sql` - Shared SQL primitives

## License

MIT