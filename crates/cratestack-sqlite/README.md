# cratestack-sqlite

The embedded facade for CrateStack: rusqlite 0.39 (SQLite on native and
`wasm32-unknown-unknown`) plus the shared schema / parser / policy / SQL
surface.

## When to use this crate

Pick `cratestack-sqlite` for **on-device storage**: mobile apps, desktop
apps, browser PWAs, Tauri shells, CLI tools, anything that runs an
embedded SQLite database in-process.

For backend services on Postgres, depend on
[`cratestack-pg`](../cratestack-pg) instead. The two crates are
strictly disjoint by design — `cratestack-sqlite` does not pull in
`sqlx` or `axum`, so it stays compatible with
`wasm32-unknown-unknown` builds.

On **native** targets `cratestack-sqlite` does re-export
`cratestack-client-rust` so hybrid consumers (NAPI / Tauri shells
that ship an embedded SQLite DB *and* call a remote backend over
HTTP) can use `include_client_schema!` alongside
`include_embedded_schema!`. The re-export is target-gated off
`wasm32` so it doesn't pull `reqwest` into browser builds.

## Installation

Schema macros emit `::cratestack::*` paths. Alias this crate as
`cratestack` via Cargo's `package =` field:

```toml
[dependencies]
cratestack = { package = "cratestack-sqlite", version = "0.4" }
```

Then in code:

```rust
cratestack::include_embedded_schema!("schema/foo.cstack");
```

## SQL views

The embedded `ViewDelegate` exposes `find_many` + `find_unique`
against an on-device `CREATE VIEW`. Materialized views are **not**
supported here — the macro's embedded composer hard-errors at
expansion time on `@@materialized` (SQLite has no materialized
views). Views declared with `@@no_unique` get a separate
`ViewDelegateNoUnique<V>` that omits `find_unique` at the type
level. See [the Views reference](https://cratestack.dev/reference/views)
and [ADR-0003](https://cratestack.dev/internals/views-adr).

## Features

- `decimal-rust-decimal` *(default)* — `Decimal` columns use `rust_decimal`.
- `decimal-bigdecimal` — alternative `bigdecimal` backend.
