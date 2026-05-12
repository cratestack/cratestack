# cratestack-rusqlite

On-device SQLite backend for offline-first applications.

## Overview

`cratestack-rusqlite` is the sync, on-device counterpart of `cratestack-sqlx`. The same `.cstack` schema that drives a Postgres service can also drive a SQLite database living on the device — phone, embedded box, or desktop app. The crate uses `rusqlite` with bundled SQLite (no system library required), no `tokio`, and no policy enforcement.

The architecture is "Rust as real frontend, Flutter (or any UI toolkit) as UI only": Rust owns state, persistence, and business logic; the UI talks to Rust over FFI. The `ffi` module ships the envelope types needed for that boundary.

`@@allow` / `@@deny` policies are **not enforced** by this backend. The device is single-user; authorization is the host app's concern.

## Installation

```toml
[dependencies]
cratestack-rusqlite = "0.2.2"
```

Or via the facade crate (recommended when sharing schema with the server):

```toml
[dependencies]
cratestack = "0.2.2"
```

## Schema

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

## Runtime

```rust
use cratestack::{RusqliteRuntime, include_schema, rusqlite_backend::ddl::create_table_sql};
use cratestack_rusqlite::ModelDelegate;

include_schema!("schema.cstack");

let runtime = RusqliteRuntime::open("app.db")?;
runtime.with_connection(|conn| {
    conn.execute_batch(&create_table_sql(&cratestack_schema::NOTE_MODEL))?;
    Ok(())
})?;
```

## CRUD

```rust
use cratestack_schema::note;

let notes = ModelDelegate::new(&runtime, &cratestack_schema::NOTE_MODEL);

// Create
let created = notes
    .create(CreateNoteInput { /* ... */ })
    .run()?;

// Find with filters and ordering
let pinned = notes
    .find_many()
    .where_expr(note::pinned().is_true())
    .order_by(note::createdAt().desc())
    .limit(20)
    .run()?;

// Find one
let row = notes.find_unique(created.id.clone()).run()?;

// Update
notes
    .update(created.id.clone())
    .set(UpdateNoteInput { title: Some("Updated".into()), ..Default::default() })
    .run()?;

// Delete
notes.delete(created.id).run()?;
```

`where_(Filter)` accepts a single filter; `where_expr(FilterExpr)` accepts the grouped AST emitted by generated field helpers.

## Storage Mapping

| Scalar     | SQLite storage                                  |
|------------|-------------------------------------------------|
| `String`   | TEXT                                            |
| `Int`      | INTEGER                                         |
| `Float`    | REAL                                            |
| `Bool`     | INTEGER (0/1)                                   |
| `Bytes`    | BLOB                                            |
| `Uuid`     | TEXT (canonical hyphenated)                     |
| `DateTime` | TEXT (RFC 3339 UTC)                             |
| `Json`     | TEXT (compact `serde_json`)                     |
| `Decimal`  | TEXT (canonical string, exact precision)        |

`@@soft_delete` is supported — DELETE rewrites to UPDATE, and `find_*` filters out soft-deleted rows.

## FFI Bridge

The `ffi` module provides the request/response envelope used at the FFI boundary:

```rust
use cratestack_rusqlite::ffi::{
    OperationRequest, OperationResponse, json_request_from, json_response_into,
};

fn ffi_call(runtime: &cratestack::RusqliteRuntime, bytes: &[u8]) -> Vec<u8> {
    let request: OperationRequest = match json_request_from(bytes) {
        Ok(req) => req,
        Err(error) => {
            let response = OperationResponse::err("bad_request", error.to_string());
            return json_response_into(&response);
        }
    };
    // Dispatch `request.model` + `request.kind` against your generated delegates,
    // then return either OperationResponse::ok(&value)? or
    // OperationResponse::err("code", "message").
    json_response_into(&OperationResponse::ok(&serde_json::json!({}))
        .unwrap_or_else(|err| OperationResponse::err("serialize", err.to_string())))
}
```

`OperationKind` variants: `FindMany`, `FindUnique`, `Create`, `Update`, `Delete`. `OperationResponse::ok(value)` JSON-encodes `value` into the `Ok { data }` variant; the `RusqliteError` `From` impl maps the storage-layer errors (`NotFound`, `Locked`, `Sqlite`) to typed `Err` codes.

The actual cdylib and `flutter_rust_bridge` glue live in the host mobile app — `cratestack-rusqlite` only owns the storage layer and the envelope.

## See Also

- [Offline-First with SQLite](https://cratestack.dev/guides/offline-first-sqlite)
- `cratestack-sqlx` — Postgres backend (server)
- `cratestack-sql` — shared SQL primitives
- `cratestack-client-flutter` — Rust-side Flutter runtime bridge

## License

MIT
