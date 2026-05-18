# cratestack-sqlite

The embedded facade for CrateStack: rusqlite 0.39 (SQLite on native and
`wasm32-unknown-unknown`) plus the shared schema / parser / policy / SQL
surface.

## When to use this crate

Pick `cratestack-sqlite` for **on-device storage**: mobile apps, desktop
apps, browser PWAs, Tauri shells, CLI tools, anything that runs an
embedded SQLite database in-process.

For backend services on Postgres, depend on
[`cratestack-pg`](../cratestack-pg) instead. The two crates are strictly
disjoint by design — `cratestack-sqlite` does not pull in `sqlx`,
`axum`, or the generated HTTP client runtime, so it stays compatible
with `wasm32-unknown-unknown` builds.

## Installation

Schema macros emit `::cratestack::*` paths. Alias this crate as
`cratestack` via Cargo's `package =` field:

```toml
[dependencies]
cratestack = { package = "cratestack-sqlite", version = "0.4" }
```

Then in code:

```rust
cratestack::include_embedded_schema!("schema/foo.cstack", db = Sqlite);
```

## Features

- `decimal-rust-decimal` *(default)* — `Decimal` columns use `rust_decimal`.
- `decimal-bigdecimal` — alternative `bigdecimal` backend.
