# cratestack

Schema-first Rust framework for typed HTTP APIs, generated clients, and backend services.

## Overview

CrateStack turns a single `.cstack` schema file into a fully-typed server and optional on-device storage layer:

- **Compile-time schema validation** via `include_schema!`
- **Generated delegates** for SQLx (Postgres) and rusqlite (SQLite on-device)
- **Generated Axum routes** for CRUD and custom procedures
- **Generated clients** for Rust, Dart, and TypeScript
- **Banking-grade primitives**: idempotency, audit log, optimistic locking, rate limiting, soft delete

## Installation

```toml
[dependencies]
cratestack = "0.2"
```

Select a Decimal backend (required):

```toml
[features]
default = ["decimal-rust-decimal"]
# or for arbitrary precision:
# decimal-bigdecimal = ["cratestack/decimal-bigdecimal"]
```

## Quickstart

### 1. Define a schema

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

### 2. Include the schema

```rust
use cratestack::include_schema;

include_schema!("schema.cstack");
```

### 3. Build the runtime

```rust
let pool = sqlx::PgPool::connect(&std::env::var("DATABASE_URL")?).await?;
let cool = cratestack_schema::CrateStack::builder(pool).build();
```

### 4. Use delegates directly

```rust
// Query with filters
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

### 5. Mount generated routes (optional)

```rust
let app = axum::Router::new().nest(
    "/api",
    cratestack_schema::axum::model_router(cool.clone(), CborCodec, AppAuthProvider),
);
```

## Two Backends

| Backend | Crate | Use Case |
|---------|-------|----------|
| Postgres | `cratestack-sqlx` | Server-side, async, policy enforcement |
| SQLite | `cratestack-rusqlite` | On-device, sync, offline-first mobile |

Both consume the same `.cstack` schema and shared primitives from `cratestack-sql`.

## Banking-Grade Primitives

All opt-in:

- **Idempotency**: `IdempotencyLayer` prevents duplicate execution under retries
- **Optimistic locking**: `@version` field with ETag/If-Match round-trip
- **Audit log**: `@@audit` model attribute with pluggable `AuditSink`
- **Rate limiting**: `RateLimitLayer` per principal
- **Soft delete**: `@@soft_delete` model attribute
- **Transaction isolation**: `@isolation("serializable")` on procedures

## Offline-First Mobile

The same schema compiles for on-device SQLite:

```rust
let runtime = cratestack::RusqliteRuntime::open("app.db")?;
let notes = ModelDelegate::new(&runtime, &cratestack_schema::NOTE_MODEL);

let created = notes.create(input).run()?; // sync, no tokio
```

## Workspace Crates

| Crate | Purpose |
|-------|---------|
| `cratestack-core` | Core types: `CoolError`, `CoolContext`, `Schema`, etc. |
| `cratestack-parser` | `.cstack` parser and validator |
| `cratestack-macros` | `include_schema!` proc-macro |
| `cratestack-policy` | Policy predicate types |
| `cratestack-sql` | Dialect-agnostic SQL primitives |
| `cratestack-sqlx` | Postgres delegates (async) |
| `cratestack-rusqlite` | SQLite delegates (sync, on-device) |
| `cratestack-axum` | Axum route generation |
| `cratestack-codec-cbor` | CBOR codec |
| `cratestack-codec-json` | JSON codec |
| `cratestack-client-rust` | Rust HTTP client runtime |
| `cratestack-client-dart` | Dart package generator |
| `cratestack-client-typescript` | TypeScript package generator |

## Documentation

- [Quickstart](https://cratestack.dev/getting-started/quickstart)
- [Current State](https://cratestack.dev/overview/current-state)
- [Banking Readiness](https://cratestack.dev/overview/banking-readiness)
- [Auth Provider](https://cratestack.dev/guides/auth-provider)
- [Transport Architecture](https://cratestack.dev/architecture/transport-architecture)

## License

MIT