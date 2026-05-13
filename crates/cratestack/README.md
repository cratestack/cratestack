# cratestack

Schema-first Rust framework for typed HTTP APIs, generated clients, and backend services. This is the public facade crate that re-exports the workspace's runtime crates and proc-macros under a single entry point.

## Overview

CrateStack turns a single `.cstack` schema file into a fully-typed server and optional on-device storage layer:

- compile-time schema validation via three role-specific macros: `include_server_schema!`, `include_embedded_schema!`, `include_client_schema!`
- generated delegates over SQLx (Postgres, async) and rusqlite (SQLite, sync — native and `wasm32-unknown-unknown` via OPFS)
- generated Axum routes for model CRUD and procedures
- generated Rust, Dart, and TypeScript clients
- opt-in primitives for banking-style workloads: idempotency, audit log, optimistic locking, rate limiting, soft delete, transaction isolation

## Installation

```toml
[dependencies]
cratestack = "0.3"
```

A `Decimal` backend feature must be selected. `decimal-rust-decimal` is the default; `decimal-bigdecimal` is reserved and not yet implemented.

For FIPS-validated TLS, enable `crypto-aws-lc-rs` (the binding glue still lives in the host service — `install_fips_crypto_provider()` surfaces a clear error when the feature is missing).

## Quickstart

### Define a schema

```cstack
auth Principal {
  id String
  role String?
}

mixin AuditFields {
  createdAt DateTime @default(dbgenerated())
  updatedAt DateTime @default(dbgenerated())
}

model Post {
  @use(AuditFields)

  id String @id
  title String
  published Boolean @default(false)
  authorId String

  @@allow("read", auth() != null)
  @@allow("update", auth().id == authorId)
}
```

### Include the schema

Pick the macro that matches your deployment:

```rust
// Server (Postgres via sqlx) — full ORM, axum routes, procedures, events
use cratestack::include_server_schema;
include_server_schema!("schema.cstack", db = Postgres);
```

```rust
// Embedded (rusqlite) — works native AND on `wasm32-unknown-unknown` via OPFS
use cratestack::include_embedded_schema;
include_embedded_schema!("schema.cstack");
```

```rust
// HTTP client — model/input stubs only, no DB
use cratestack::include_client_schema;
include_client_schema!("schema.cstack");
```

The three macros emit a `cratestack_schema` module each — mutually exclusive within a single crate. Pick one per crate based on the crate's role.

### Build the runtime

```rust
let pool = sqlx::PgPool::connect(&std::env::var("DATABASE_URL")?).await?;
let cool = cratestack_schema::Cratestack::builder(pool).build();
```

### Use delegates

```rust
use cratestack::CoolContext;

let ctx = CoolContext::anonymous();

let posts = cool
    .post()
    .find_many()
    .where_expr(
        cratestack_schema::post::published().is_true()
            .and(cratestack_schema::post::author().email().eq("owner@example.com"))
    )
    .order_by(cratestack_schema::post::createdAt().desc())
    .limit(20)
    .run(&ctx)
    .await?;
```

`bind_auth` / `bind_context` produce a `BoundCratestack` that captures the auth context once and drops the trailing `ctx` argument on each call.

### Mount generated routes

```rust
use cratestack::axum;
use cratestack_codec_cbor::CborCodec;

let app = axum::Router::new().nest(
    "/api",
    cratestack_schema::axum::model_router(cool.clone(), CborCodec, AppAuthProvider),
);
```

## Two Backends

| Backend  | Crate                  | Use case                                                          |
|----------|------------------------|-------------------------------------------------------------------|
| Postgres | `cratestack-sqlx`      | Server-side, async, full policy enforcement                       |
| SQLite   | `cratestack-rusqlite`  | On-device sync (mobile/desktop) AND in-browser via OPFS (wasm32)  |

Both consume the same `.cstack` schema and share the primitives in `cratestack-sql`. `cratestack-rusqlite 0.3+` compiles to `wasm32-unknown-unknown` via [`sqlite-wasm-rs`](https://crates.io/crates/sqlite-wasm-rs); call `cratestack_rusqlite::opfs::install_opfs_vfs(...)` inside a Dedicated Worker before opening the connection.

## Banking-Grade Primitives

All opt-in:

- **Idempotency** — `IdempotencyLayer` (cratestack-axum) plus `SqlxIdempotencyStore` for at-most-once execution under retries
- **Optimistic locking** — `@version` field with `If-Match` / `ETag` round-trip and `if_match(...)` on update builders
- **Audit log** — `@@audit` on a model, written inside the same transaction as the mutation; fan-out via `AuditSink`
- **Rate limiting** — `RateLimitLayer` per principal (cratestack-axum)
- **Soft delete** — `@@soft_delete` model attribute
- **Transaction isolation** — `@isolation("serializable")` on procedures, plus `run_in_isolated_tx` / `run_in_isolated_tx_with_retries`

## Offline-First Mobile

The same schema compiles for on-device SQLite. `cratestack-rusqlite` is a sync API with no `tokio` and no policy enforcement (the device is single-user):

```rust
use cratestack::{RusqliteRuntime, rusqlite_backend::ddl::create_table_sql};
use cratestack_rusqlite::ModelDelegate;

let runtime = RusqliteRuntime::open("app.db")?;
runtime.with_connection(|conn| {
    conn.execute_batch(&create_table_sql(&cratestack_schema::NOTE_MODEL))?;
    Ok(())
})?;

let notes = ModelDelegate::new(&runtime, &cratestack_schema::NOTE_MODEL);
let created = notes.create(/* CreateNoteInput { ... } */).run()?;
```

## Workspace Crates

| Crate                            | Purpose                                                    |
|----------------------------------|------------------------------------------------------------|
| `cratestack-core`                | Core types: `CoolError`, `CoolContext`, `Schema`, audit    |
| `cratestack-parser`              | `.cstack` parser and semantic checker                      |
| `cratestack-macros`              | `include_server_schema!`, `include_embedded_schema!`, `include_client_schema!` |
| `cratestack-policy`              | Policy literal/predicate types and procedure evaluation    |
| `cratestack-sql`                 | Dialect-agnostic SQL primitives shared by both backends    |
| `cratestack-sqlx`                | Postgres delegates (async)                                 |
| `cratestack-rusqlite`            | SQLite delegates (sync, on-device)                         |
| `cratestack-axum`                | Axum route generation, idempotency/ratelimit middleware    |
| `cratestack-codec-cbor`          | CBOR codec                                                 |
| `cratestack-codec-json`          | JSON codec                                                 |
| `cratestack-client-rust`         | Rust HTTP client runtime                                   |
| `cratestack-client-dart`         | Dart package generator                                     |
| `cratestack-client-typescript`   | TypeScript package generator                               |
| `cratestack-redis`               | Redis-backed idempotency store                             |

## Documentation

- [Quickstart](https://cratestack.dev/getting-started/quickstart)
- [Current State](https://cratestack.dev/overview/current-state)
- [Banking Readiness](https://cratestack.dev/overview/banking-readiness)
- [Auth Provider](https://cratestack.dev/guides/auth-provider)
- [Transport Architecture](https://cratestack.dev/architecture/transport-architecture)
- [Offline-First SQLite](https://cratestack.dev/guides/offline-first-sqlite)

## License

MIT
