# cratestack-rusqlite

Embedded SQLite backend for offline-first applications. Works on **native (mobile, desktop)** and on **`wasm32-unknown-unknown` (browser via OPFS)**.

## Overview

`cratestack-rusqlite` is the sync, embedded counterpart of `cratestack-sqlx`. The same `.cstack` schema that drives a Postgres service can also drive a SQLite database living on the device — phone, embedded box, desktop app, **or browser tab**. The crate uses `rusqlite 0.39` with bundled SQLite (no system library required), no `tokio`, and no policy enforcement.

Since `rusqlite 0.39`, the same crate transparently swaps its FFI backend per target:

- **Native** (Linux/macOS/Windows/iOS/Android): `libsqlite3-sys` with bundled SQLite.
- **`wasm32-unknown-unknown`**: `sqlite-wasm-rs` with sync-access OPFS persistence.

`@@allow` / `@@deny` policies are **not enforced** by this backend. Clients are untrusted; authorization is the server's concern.

The architecture is "Rust as real frontend, UI layer is UI only": Rust owns state, persistence, and business logic; the UI talks to Rust over FFI or `wasm-bindgen`. The `ffi` module ships the JSON envelope types needed for that boundary.

## Installation

```toml
[dependencies]
cratestack-rusqlite = "0.3"
cratestack-macros = "0.3"  # for include_embedded_schema!
```

Or via the facade crate (recommended when sharing schema with the server):

```toml
[dependencies]
cratestack = "0.3"
```

### Building for the browser (`wasm32-unknown-unknown`)

`sqlite-wasm-rs` compiles SQLite's C source to WebAssembly via `cc-rs`, which requires a wasm-capable clang on `PATH`. Apple's stock Xcode clang does **not** include the wasm32 backend.

**macOS:**

```bash
brew install llvm
export CC=/opt/homebrew/opt/llvm/bin/clang   # Apple Silicon
# export CC=/usr/local/opt/llvm/bin/clang    # Intel
```

**Linux:**

```bash
sudo apt-get install clang lld   # Clang 14+
```

Or use the Emscripten SDK (`emsdk`) and point `CC` at `emcc`.

Then:

```bash
cargo build -p cratestack-rusqlite --target wasm32-unknown-unknown
```

### Browser persistence via OPFS

OPFS `SyncAccessHandle` is **only available inside a Dedicated Worker** per the spec — install the VFS there before opening the connection:

```rust
// Inside a Dedicated Worker context
use cratestack_rusqlite::{RusqliteRuntime, opfs};

opfs::install_opfs_vfs(&opfs::OpfsOptions::default()).await?;
let runtime = RusqliteRuntime::open("app.db")?;
```

Main-thread code can still use `RusqliteRuntime::open_in_memory()` for ephemeral state.

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
use cratestack::{RusqliteRuntime, include_embedded_schema, rusqlite_backend::ddl::create_table_sql};
use cratestack_rusqlite::ModelDelegate;

include_embedded_schema!("schema.cstack");

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
