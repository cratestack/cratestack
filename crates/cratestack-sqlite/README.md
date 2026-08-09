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
cratestack = { package = "cratestack-sqlite", version = "0.6.7" }
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
- `decimal-bigdecimal` — arbitrary-precision `bigdecimal` backend instead
  (heap-allocated, not `Copy` — see `cratestack-core`'s README for the
  trait differences). Mutually exclusive with `decimal-rust-decimal`;
  selecting neither or both is a compile error. **Wire compatibility
  constraint:** ordinary values encode identically to `rust_decimal` on
  the wire, but values past `rust_decimal`'s ~28-29 significant-digit
  capacity serialize as scientific notation (e.g. `"1E-29"`), which a
  `rust_decimal` peer fails to decode. The shipped Dart/TypeScript client
  SDKs generate a real arbitrary-precision `Decimal` type as of
  cratestack#498 (**breaking** — see each package's migration note) that
  parses both notations, so pairing this backend with a generated client
  is safe regardless of magnitude — see `cratestack-core`'s README for
  the full picture and the two scope notes (gRPC-preset clients, the
  TypeScript `swr` preset) that still apply.
- `codec-json` *(default)* — forwards the JSON codec to the generated
  client runtime, alongside CBOR. On `wasm32` the client runtime isn't
  linked (no `reqwest`), so this feature has no effect there.
