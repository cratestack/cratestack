# cratestack-pg

The server-side facade for CrateStack: Postgres (via `sqlx`), Axum HTTP
bindings, the generated Rust client runtime, and the shared schema /
parser / policy / SQL surface.

## When to use this crate

Pick `cratestack-pg` for **backend services**: HTTP servers, background
workers, anything that needs the sqlx Postgres runtime, generated Axum
routes, or the in-process generated Rust client.

For embedded / mobile / wasm targets (rusqlite, SQLite, `wasm32`), depend
on [`cratestack-sqlite`](../cratestack-sqlite) instead. The two crates
are strictly disjoint by design — `cratestack-pg` does not pull in
`libsqlite3-sys`, which lets you depend on the official `sqlx` umbrella
crate alongside it without tripping Cargo's `links = "sqlite3"`
collision rule.

## Installation

Schema macros emit `::cratestack::*` paths. Alias this crate as
`cratestack` via Cargo's `package =` field:

```toml
[dependencies]
cratestack = { package = "cratestack-pg", version = "0.4" }
```

Then in code:

```rust
cratestack::include_server_schema!("schema/foo.cstack", db = Postgres);
```

## SQL views

This facade re-exports `ViewDelegate` and `ViewDelegateNoUnique` — the
read-only delegates handed out by `runtime.views().<view_snake>()`
for every `view` block in the schema. `ViewDelegate` exposes
`find_many` + `find_unique` and, on `@@materialized` views, a
`refresh()` method that runs `REFRESH MATERIALIZED VIEW CONCURRENTLY`.
`ViewDelegateNoUnique` (for `@@no_unique` views) exposes only
`find_many` — `find_unique` and `refresh()` are absent at the type
level. See [the Views reference](https://cratestack.dev/reference/views)
and [ADR-0003](https://cratestack.dev/internals/views-adr).

## Features

- `decimal-rust-decimal` *(default)* — `Decimal` columns use `rust_decimal`.
- `decimal-bigdecimal` — alternative `bigdecimal` backend.
- `crypto-aws-lc-rs` — opt into the `aws-lc-rs` rustls provider for
  FIPS-validated deployments. See `install_fips_crypto_provider()`.
