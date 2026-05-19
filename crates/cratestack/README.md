# CrateStack

**Rust-native, schema-first framework for typed HTTP APIs, generated clients, and backend services.** Write one `.cstack` schema; the framework emits the server, the embedded SQLite slice, and the typed Rust / Dart / TypeScript clients from the same source of truth.

> This crate is a **documentation-only landing page** for the framework. It exports no items.
> Depend on one of the two facade crates below depending on what you're building.

## Pick a facade

```toml
# Backend service — Postgres (sqlx) + Axum HTTP bindings + generated
# Rust client runtime. The shape you want for an HTTP service that
# owns its own database.
cratestack = { package = "cratestack-pg", version = "0.4" }

# Embedded — rusqlite-backed SQLite on native (mobile, desktop) and
# wasm32-unknown-unknown (browser, OPFS-backed). The shape you want
# for an on-device storage layer that ships with a host app.
cratestack = { package = "cratestack-sqlite", version = "0.4" }
```

The two facades are **strictly disjoint by design**. `cratestack-pg` does not pull in `libsqlite3-sys`, so backend services can keep depending on the official `sqlx` umbrella alongside it without `links = "sqlite3"` conflicts. `cratestack-sqlite` does not pull in `sqlx` or `axum`, so the embedded slice compiles to wasm without forcing every consumer onto a tokio-net dep graph.

Both crates expose their library as `cratestack` (the schema macros emit `::cratestack::*` paths), so the rename via Cargo's `package =` field is invisible inside your code.

## What you get from one `.cstack` file

* **Server** — sqlx + axum CRUD routes, procedures, policies, projections, audit log, idempotency, rate limiting, transaction isolation control, materialized views.
* **Embedded** — same schema, rusqlite delegate, sync API, identical scalar round-tripping (`Decimal`, `Uuid`, `DateTime`, `Json` through canonical TEXT storage). One source, three targets (native mobile, desktop, wasm).
* **Typed clients** — generated Rust client (CBOR by default, optional JSON), Dart package, TypeScript package, each consuming the same canonical HTTP contract.
* **SQL views** — `view <Name> from <Model>, ...` produces a typed Rust struct and `ViewDelegate`, with per-backend SQL bodies and optional `@@materialized` (Postgres only).
* **Banking-readiness primitives** — `@version` optimistic locking, `@@audit`, `IdempotencyLayer`, `RateLimitLayer`, FIPS-validated TLS via `crypto-aws-lc-rs`, soft delete, transactional audit log.

See the [Current State](https://cratestack.dev/overview/current-state) page for the authoritative feature matrix.

## Quickstart

A minimal `schema.cstack`:

```cstack
datasource db {
  provider = "postgresql"
}

model Post {
  id      Uuid     @id @default(uuid())
  title   String
  body    String
  authorId Uuid

  @@allow("read", auth() != null)
  @@allow("create", auth() != null && authorId == auth().id)
}
```

A server consuming it:

```rust
use cratestack::include_server_schema;

include_server_schema!("schema.cstack", db = Postgres);
```

The macro emits `CrateStackClient` with typed `posts().create(...)`, `posts().find_many()...run(&ctx).await`, and an Axum router you can mount. Same `schema.cstack` works with `include_embedded_schema!` against `cratestack-sqlite` for an on-device store, or with `include_client_schema!` for a Rust HTTP client.

Full walkthrough: <https://cratestack.dev/getting-started/quickstart>.

## Where to read more

* **Documentation site** — <https://cratestack.dev>
* **Rust API docs** — <https://rust-doc.cratestack.dev/cratestack>
* **Source repository** — <https://github.com/cratestack/cratestack>
* **Architecture decision records** — <https://cratestack.dev/internals/core-architecture-adr>

## License

MIT. See [LICENSE](https://github.com/cratestack/cratestack/blob/main/LICENSE).
