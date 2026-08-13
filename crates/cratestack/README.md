# CrateStack

**Rust-native, schema-first framework for typed HTTP APIs, generated clients, and backend services.** Write one `.cstack` schema; the framework emits the server, the embedded SQLite slice, and the typed Rust / Dart / TypeScript clients from the same source of truth.

> This crate is a **documentation-only landing page** for the framework. It exports no items.
> Depend on one of the facade crates below depending on what you're building.

## Pick a facade

```toml
# Backend service — Postgres (sqlx) + Axum HTTP bindings + generated
# Rust client runtime. The shape you want for an HTTP service that
# owns its own database.
cratestack = { package = "cratestack-pg", version = "0.7" }

# Procedures-only, no-database backend service — Axum HTTP bindings +
# generated Rust client runtime, with `sqlx` genuinely absent from the
# dependency graph. The shape you want for a pure RPC/REST facade, a
# stateless computation endpoint, or a gateway with no persisted models.
cratestack = { package = "cratestack-api", version = "0.7" }

# Embedded — rusqlite-backed SQLite on native (mobile, desktop) and
# wasm32-unknown-unknown (browser, OPFS-backed). The shape you want
# for an on-device storage layer that ships with a host app.
cratestack = { package = "cratestack-sqlite", version = "0.7" }

# Pure HTTP-client SDK — include_client_schema! plus the generated
# Rust client runtime, with `cratestack-axum` (and therefore
# `axum`/`tower`/`hyper`/`tower-http`) structurally absent from the
# dependency graph. The shape you want for a crate that only ever
# calls a cratestack server, never runs one.
cratestack = { package = "cratestack-client", version = "0.7" }
```

The four facades are **strictly disjoint by design** — no shared "backend" trait between them. `cratestack-pg` does not pull in `libsqlite3-sys`, so backend services can keep depending on the official `sqlx` umbrella alongside it without `links = "sqlite3"` conflicts. `cratestack-api` never depends on `cratestack-sqlx` at all (see [`docs/design/no-database-mode.md`](https://github.com/cratestack/cratestack/blob/main/docs/design/no-database-mode.md) §7) — pick it for `db = None` schemas instead of `cratestack-pg` with `default-features = false` (which still works, and isn't going away). `cratestack-sqlite` does not pull in `sqlx` or `axum`, so the embedded slice compiles to wasm without forcing every consumer onto a tokio-net dep graph. `cratestack-client` re-exports **only** `include_client_schema!` (not the other two entry macros) and does not pull in `cratestack-axum` at all, under any of its own features (see [cratestack#490](https://github.com/cratestack/cratestack/issues/490)) — pick it over `cratestack-pg`/`cratestack-api` when the crate never runs a server.

All four crates expose their library as `cratestack` (the schema macros emit `::cratestack::*` paths), so the rename via Cargo's `package =` field is invisible inside your code.

## What you get from one `.cstack` file

* **Server** — sqlx + axum CRUD routes, procedures, policies, projections, audit log, idempotency, rate limiting, transaction isolation control, materialized views.
* **Embedded** — same schema, rusqlite delegate, sync API, identical scalar round-tripping (`Decimal`, `Uuid`, `DateTime`, `Json` through canonical TEXT storage). One source, three targets (native mobile, desktop, wasm).
* **Typed clients** — generated Rust client (CBOR by default, optional JSON), Dart package, TypeScript package, each consuming the same canonical HTTP contract.
* **`transport grpc`** — an alternative to REST/RPC transport: `.proto` messages (with a field-number lockfile), a tonic service, and gRPC clients (Rust, Dart native, TypeScript gRPC-Web). Covers model CRUD (#171) and `procedure`s, unary or server-streaming (#208). Both `include_server_schema!` and `include_client_schema!` need `cratestack-pg`'s `grpc` Cargo feature enabled to emit real gRPC codegen instead of a `compile_error!`. **Planned for removal in v0.9** — don't build new integrations against it. See [`docs/design/protobuf.md`](https://github.com/cratestack/cratestack/blob/main/docs/design/protobuf.md).
* **SQL views** — `view <Name> from <Model>, ...` produces a typed Rust struct and `ViewDelegate`, with per-backend SQL bodies and optional `@@materialized` (Postgres only).
* **Banking-readiness primitives** — `@version` optimistic locking, `@@audit`, `IdempotencyLayer`, `RateLimitLayer`, soft delete, transactional audit log. (FIPS-validated TLS via `crypto-aws-lc-rs` is reserved but not implemented yet — see [#334](https://github.com/cratestack/cratestack/issues/334).)

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
