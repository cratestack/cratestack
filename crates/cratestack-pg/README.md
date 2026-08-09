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
cratestack = { package = "cratestack-pg", version = "0.6.7" }
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

- `postgres` *(default)* — gates `dep:cratestack-sqlx` (and forwards
  `cratestack-macros/postgres`). On by default so every existing
  `db = Postgres` consumer sees zero behavior change. A consumer that only
  ever uses `db = None` schemas can disable it with
  `default-features = false` (re-adding whichever of
  `decimal-rust-decimal`/`codec-json` it still wants) to keep `sqlx` and
  its transitive dependency tree out of the build entirely — or depend on
  [`cratestack-api`](../cratestack-api) instead, which never pulls in
  `cratestack-sqlx` at all.
- `decimal-rust-decimal` *(default)* — `Decimal` columns use `rust_decimal`.
  Requires *some* decimal backend feature to be selected alongside
  `postgres` — `cratestack-sqlx`'s query-builder support code binds
  `cratestack_core::Decimal` unconditionally — but `postgres` itself no
  longer forces a specific one (it used to, before `decimal-bigdecimal`
  existed for real; forcing one unconditionally would make the other
  unreachable through this facade).
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
  client runtime, so `include_client_schema!` offers both CBOR and JSON.
- `grpc` — real `transport grpc` Rust codegen (server tonic service +
  Rust gRPC client). Off by default: pulls in `tonic`/`prost` for every
  consumer otherwise, whether or not they use gRPC. Without it, a `Grpc`
  schema fails `include_server_schema!`/`include_client_schema!` with a
  `compile_error!` pointing here.
- `crypto-aws-lc-rs` — **not implemented yet**, enabling it is a hard
  `compile_error!`. Reserved for a future FIPS-validated `aws-lc-rs`
  rustls provider; see `install_fips_crypto_provider()`'s doc comment
  and [#334](https://github.com/cratestack/cratestack/issues/334).
