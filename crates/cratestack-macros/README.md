# cratestack-macros

Procedural macros for compile-time schema processing.

## Overview

`cratestack-macros` exposes three role-specific proc-macros that parse a `.cstack` file at compile time and emit Rust code inside a `cratestack_schema` module. Each macro maps to a deployment shape:

| Macro                          | Deployment shape                                                | Emits                                                  |
|--------------------------------|-----------------------------------------------------------------|--------------------------------------------------------|
| `include_server_schema!`       | Server (`db = Postgres` via sqlx, or `db = None` — procedures-only, no database) | full ORM + axum router + procedures + events + sqlx FromRow (`db = Postgres`); `PgPool`-free router + procedures only, no models/events (`db = None`) |
| `include_embedded_schema!`     | Embedded ORM (mobile / desktop / browser via OPFS)              | model structs + descriptors + rusqlite FromRow + inputs |
| `include_client_schema!`       | HTTP client (talks to a server, owns no DB)                     | model/input/procedure stubs, no DB                      |

The split is **strict**: `include_server_schema!` never emits rusqlite items, `include_embedded_schema!` never emits sqlx items. Each deployment pays only for its own surface.

`include_server_schema!` and `include_client_schema!` also accept a schema declaring `transport grpc` (instead of the default REST, or `transport rpc`) when the consuming facade's `grpc` Cargo feature is enabled; `include_embedded_schema!` rejects `Grpc` schemas unconditionally, since the embedded role has no transport at all. See [`transport grpc`](#transport-grpc) below.

All three are re-exported through the facade crates `cratestack-pg`, `cratestack-api`, and `cratestack-sqlite`; most consumers should depend on one of those (renamed to `cratestack` via Cargo's `package =` field) rather than this crate directly. The choice of facade picks which side of the strict split the macro emits against — backend services use `cratestack-pg` (or `cratestack-api` for `db = None`), embedded / mobile / wasm builds use `cratestack-sqlite`. A fourth facade, `cratestack-client` (cratestack#490), re-exports **only** `include_client_schema!` — not the other two — for consumers that are pure HTTP-client SDKs and want `cratestack-axum` structurally absent from their dependency graph.

## Installation

```toml
[dependencies]
# Server-side
cratestack = { package = "cratestack-pg", version = "0.6.7" }
# OR embedded-side
# cratestack = { package = "cratestack-sqlite", version = "0.6.7" }
```

## `include_server_schema!`

```rust
use cratestack::include_server_schema;

include_server_schema!("schema.cstack", db = Postgres);

let pool = sqlx::PgPool::connect(&database_url).await?;
let cool = cratestack_schema::Cratestack::builder(pool).build();
```

`db = Postgres` and `db = None` are currently accepted. The parser is wired so adding `MySql` / `Sqlite`-via-sqlx in a future release is non-breaking at call sites that already pass `db = Postgres`.

For `db = Postgres`, the macro emits, inside a `cratestack_schema` module:

- model structs + `sqlx::FromRow<PgRow>` impls for each `model`
- `Create<Model>Input` and `Update<Model>Input` structs
- per-model selection / include builders
- per-model filter/order helper modules (e.g. `cratestack_schema::post::published()`)
- the `Cratestack` runtime struct with `builder(pool)`, `bind_context(ctx)`, `bind_auth(principal)`, and per-model accessors (`cool.post()`, `cool.user()`, ...)
- `axum::model_router(cool, codec, auth_provider)` and `axum::procedure_router(...)`
- procedure dispatch glue and `events::Subscriptions` for `@@emit` model events
- for each `view` block: a typed struct, `<UPPER>_VIEW: ViewDescriptor<...>` const, `sqlx::FromRow<PgRow>` impl, and an accessor on `cool.views().<view_snake>()` returning `ViewDelegate` (or `ViewDelegateNoUnique` for `@@no_unique` views). `@@materialized` views also get a `refresh()` method. See [ADR-0003](https://cratestack.dev/internals/views-adr).

### `db = None` — procedures-only, no database

```rust
use cratestack::include_server_schema;

include_server_schema!("schema.cstack", db = None);
```

For a schema whose `datasource` block declares `provider = "none"`. Such a schema can **never** declare a `model` — it's cross-checked against the macro's own `db` argument, so a mismatch (`db = Postgres` against a `none` datasource, or vice versa) is a `compile_error!` rather than a silent no-op. The macro emits a genuinely `PgPool`-free `Cratestack`/router — not an `Option<PgPool>` that happens to always be `None` — with `ModelRouterState` and the event module (`events::Subscriptions`) omitted entirely rather than compiled in as dead code. Still emits `axum::procedure_router(...)` and procedure dispatch glue, since procedures are the whole point of this mode. See [`docs/design/no-database-mode.md`](https://github.com/cratestack/cratestack/blob/main/docs/design/no-database-mode.md).

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
- for each non-`@@materialized` `view` block: a typed struct, `<UPPER>_VIEW: ViewDescriptor<...>` const, and `cratestack_rusqlite::FromRusqliteRow` impl. `@@materialized` views are rejected at expansion time with a `compile_error!` referencing [ADR-0003](https://cratestack.dev/internals/views-adr) — SQLite has no materialized views.

It **does not** emit: `sqlx::FromRow`, the `Cratestack` server runtime, axum routes, procedure handlers, or events. Policies (`@@allow` / `@@deny`) are silently dropped — clients are untrusted; authorization is the server's job.

`@@audit` and `@@emit` directives are currently no-ops in this macro. The local-journal / local-event-bus implementations land in a follow-up release.

## `include_client_schema!`

Emits a strict subset of the server surface — model and input types, enums, selection/projection helpers, and the `client::Client` wrapper — with no SQLx or Axum integration. Renamed from `include_client_macro!` in 0.3.0.

```rust
use cratestack::include_client_schema;

include_client_schema!("../schemas/api.cstack");
```

## `transport grpc`

A `.cstack` schema can declare `transport grpc` instead of the default REST (or `transport rpc`) transport — mutually exclusive with both. It's rejected with a `compile_error!` in `include_server_schema!` and `include_client_schema!` unless the facade's `grpc` Cargo feature is enabled (`cratestack-pg`'s `grpc` feature — see `crates/cratestack-macros/src/include/reject_grpc.rs`); `include_embedded_schema!` rejects `Grpc` schemas unconditionally, feature or no feature, since the embedded role has no transport at all.

With the feature on, `include_server_schema!` emits `.proto`-mirroring `pb::` message structs (with `From`/`TryFrom` conversions to the domain types), a hand-rolled tonic service that delegates to the same dispatch functions REST/RPC already call (so policy/audit/idempotency behavior is unchanged), and `include_client_schema!` emits a native Rust gRPC client (`client_rust::grpc::CratestackGrpcClient`). Coverage is **CRUD-only today** — `procedure` declarations aren't wired into the generated gRPC service yet, and there's no streaming support. See [`docs/design/protobuf.md`](https://github.com/cratestack/cratestack/blob/main/docs/design/protobuf.md) and the `cratestack-cli generate-proto` subcommand for the standalone `.proto` generator.

## Migration from 0.2.x

`include_schema!` and `include_client_macro!` were removed in 0.3.0. Migrate:

| Before | After |
|---|---|
| `include_schema!("schema.cstack");` (server context) | `include_server_schema!("schema.cstack", db = Postgres);` |
| `include_schema!("schema.cstack");` (sqlite/embedded context) | `include_embedded_schema!("schema.cstack");` |
| `include_client_macro!("schema.cstack");` | `include_client_schema!("schema.cstack");` |

See the workspace `CHANGELOG.md` for full release notes.

## Decimal Backend

Generated code references `cratestack::Decimal`, which resolves at compile time to either `rust_decimal::Decimal` (`decimal-rust-decimal`, default) or `bigdecimal::BigDecimal` (`decimal-bigdecimal`) — whichever backend the facade the schema is compiled into selected. The two are mutually exclusive: selecting both is a compile error in `cratestack-core`. Selecting neither is allowed as of cratestack#521 — `Decimal` is simply not exported, so generated code referencing it fails with rustc's own "cannot find type `Decimal`".

## See Also

- `cratestack` — facade crate that re-exports the macros
- `cratestack-parser` — the parser the macros call
- [Quickstart](https://cratestack.dev/getting-started/quickstart)

## License

MIT
