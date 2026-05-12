# cratestack-macros

Procedural macros for compile-time schema processing.

## Overview

`cratestack-macros` exposes three role-specific proc-macros that parse a `.cstack` file at compile time and emit Rust code inside a `cratestack_schema` module. Each macro maps to a deployment shape:

| Macro                          | Deployment shape                                                | Emits                                                  |
|--------------------------------|-----------------------------------------------------------------|--------------------------------------------------------|
| `include_server_schema!`       | Server (Postgres via sqlx)                                      | full ORM + axum router + procedures + events + sqlx FromRow |
| `include_embedded_schema!`     | Embedded ORM (mobile / desktop / browser via OPFS)              | model structs + descriptors + rusqlite FromRow + inputs |
| `include_client_schema!`       | HTTP client (talks to a server, owns no DB)                     | model/input/procedure stubs, no DB                      |

The split is **strict**: `include_server_schema!` never emits rusqlite items, `include_embedded_schema!` never emits sqlx items. Each deployment pays only for its own surface.

All three are re-exported through the facade `cratestack` crate; most consumers should depend on `cratestack` rather than this crate directly. Embedded-only consumers building for `wasm32` may prefer to depend on `cratestack-macros` + `cratestack-rusqlite` directly to shed the server-side transitive deps.

## Installation

```toml
[dependencies]
cratestack = "0.3"
```

## `include_server_schema!`

```rust
use cratestack::include_server_schema;

include_server_schema!("schema.cstack", db = Postgres);

let pool = sqlx::PgPool::connect(&database_url).await?;
let cool = cratestack_schema::Cratestack::builder(pool).build();
```

Only `db = Postgres` is currently accepted. The parser is wired so adding `MySql` / `Sqlite`-via-sqlx in a future release is non-breaking at call sites that already pass `db = Postgres`.

The macro emits, inside a `cratestack_schema` module:

- model structs + `sqlx::FromRow<PgRow>` impls for each `model`
- `Create<Model>Input` and `Update<Model>Input` structs
- per-model selection / include builders
- per-model filter/order helper modules (e.g. `cratestack_schema::post::published()`)
- the `Cratestack` runtime struct with `builder(pool)`, `bind_context(ctx)`, `bind_auth(principal)`, and per-model accessors (`cool.post()`, `cool.user()`, ...)
- `axum::model_router(cool, codec, auth_provider)` and `axum::procedure_router(...)`
- procedure dispatch glue and `events::Subscriptions` for `@@emit` model events

## `include_embedded_schema!`

```rust
use cratestack::include_embedded_schema;
use cratestack_rusqlite::{ModelDelegate, RusqliteRuntime, rusqlite_backend::ddl::create_table_sql};

include_embedded_schema!("schema.cstack");

let runtime = RusqliteRuntime::open("app.db")?;
runtime.with_connection(|conn| {
    conn.execute_batch(&create_table_sql(&cratestack_schema::NOTE_MODEL))?;
    Ok(())
})?;

let notes = ModelDelegate::new(&runtime, &cratestack_schema::NOTE_MODEL);
```

The macro emits:

- model structs + `cratestack_rusqlite::FromRusqliteRow` impls
- `ModelDescriptor` constants (needed by `ModelDelegate`)
- `Create<Model>Input` / `Update<Model>Input` with `CreateModelInput` / `UpdateModelInput` impls
- per-model filter helper modules

It **does not** emit: `sqlx::FromRow`, the `Cratestack` server runtime, axum routes, procedure handlers, or events. Policies (`@@allow` / `@@deny`) are silently dropped — clients are untrusted; authorization is the server's job.

`@@audit` and `@@emit` directives are currently no-ops in this macro. The local-journal / local-event-bus implementations land in a follow-up release.

## `include_client_schema!`

Emits a strict subset of the server surface — model and input types, enums, selection/projection helpers, and the `client::Client` wrapper — with no SQLx or Axum integration. Renamed from `include_client_macro!` in 0.3.0.

```rust
use cratestack::include_client_schema;

include_client_schema!("../schemas/api.cstack");
```

## Migration from 0.2.x

`include_schema!` and `include_client_macro!` were removed in 0.3.0. Migrate:

| Before | After |
|---|---|
| `include_schema!("schema.cstack");` (server context) | `include_server_schema!("schema.cstack", db = Postgres);` |
| `include_schema!("schema.cstack");` (sqlite/embedded context) | `include_embedded_schema!("schema.cstack");` |
| `include_client_macro!("schema.cstack");` | `include_client_schema!("schema.cstack");` |

See the workspace `CHANGELOG.md` for full release notes.

## Decimal Backend

Generated code references `cratestack::Decimal`, which resolves at compile time to either `rust_decimal::Decimal` (`decimal-rust-decimal`, default) or the reserved `decimal-bigdecimal` backend.

## See Also

- `cratestack` — facade crate that re-exports the macros
- `cratestack-parser` — the parser the macros call
- [Quickstart](https://cratestack.dev/getting-started/quickstart)

## License

MIT
