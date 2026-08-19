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
cratestack = { package = "cratestack-pg", version = "0.7" }
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
- `decimal-rust-decimal` *(default)* — makes `rust_decimal`-backed
  `Decimal` columns available (`::cratestack::RustDecimal`). A schema picks
  which concrete backend it uses per macro call, via the
  `decimal = RustDecimal | BigDecimal` argument on `include_server_schema!`
  (cratestack#505 Direction 2) — this feature just needs to be enabled
  (alongside `postgres`) for any schema in the build to be able to name
  `RustDecimal`; `postgres` itself no longer forces a specific one (it used
  to, before `decimal-bigdecimal` existed for real).
- `decimal-bigdecimal` — makes the arbitrary-precision `bigdecimal` backend
  available instead (`::cratestack::BigDecimal`; heap-allocated, not
  `Copy` — see `cratestack-core`'s README for the trait differences). As of
  cratestack#505 Direction 2, this is **not** mutually exclusive with
  `decimal-rust-decimal` — both may be enabled at once, and two different
  schemas in the same build (or even the same schema's dependents) can each
  pick the backend they asked for; see `cratestack-core`'s README and
  `docs/design/decimal-backend-additivity.md` for the mechanism. Selecting
  *neither* is still allowed as of cratestack#521 — the `Decimal` type is
  simply not exported, so only a schema that actually uses a `Decimal`
  field fails, with rustc's own "cannot find type `Decimal`" (or, for a
  schema that omits the now-required `decimal = ...` macro argument, a
  clear compile error naming exactly what to add). **Wire compatibility
  constraint:** ordinary values encode identically to `rust_decimal` on
  the wire, but values past `rust_decimal`'s ~28-29 significant-digit
  capacity serialize as scientific notation (e.g. `"1E-29"`), which a
  `rust_decimal` peer fails to decode. The shipped Dart/TypeScript client
  SDKs generate a real arbitrary-precision `Decimal` type as of
  cratestack#498 (**breaking** — see each package's migration note) that
  parses both notations, so pairing this backend with a generated client
  is safe regardless of magnitude — see `cratestack-core`'s README for
  the full picture and the TypeScript `swr` preset scope note that still
  applies.
- `codec-json` *(default)* — forwards the JSON codec to the generated
  client runtime, so `include_client_schema!` offers both CBOR and JSON.
- `crypto-aws-lc-rs` — **not implemented yet**, enabling it is a hard
  `compile_error!`. Reserved for a future FIPS-validated `aws-lc-rs`
  rustls provider; see `install_fips_crypto_provider()`'s doc comment
  and [#334](https://github.com/cratestack/cratestack/issues/334).
