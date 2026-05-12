# cratestack-macros

Procedural macros for compile-time schema processing.

## Overview

`cratestack-macros` exposes two proc-macros that parse a `.cstack` file at compile time and emit Rust code inside a `cratestack_schema` module:

- `include_schema!("path.cstack")` — full server surface
- `include_client_macro!("path.cstack")` — client surface only

Both are re-exported through the facade `cratestack` crate; consumers should depend on `cratestack` rather than this crate directly.

## Installation

```toml
[dependencies]
cratestack = "0.2.2"
```

## `include_schema!`

```rust
use cratestack::include_schema;

include_schema!("schema.cstack");

let pool = sqlx::PgPool::connect(&database_url).await?;
let cool = cratestack_schema::Cratestack::builder(pool).build();
```

The macro emits, inside a `cratestack_schema` module:

- model structs for each `model`/`type`/`enum` declaration
- `Create<Model>Input` and `Update<Model>Input` structs for each model
- per-model selection and include builders (`Model::select()`, `Model::include_selection()`)
- per-model filter/order helper modules (e.g. `cratestack_schema::post::published()`)
- the `Cratestack` runtime struct with `builder(pool)`, `build()`, `bind_context(ctx)`, `bind_auth(principal)`, and a per-model accessor (`cool.post()`, `cool.user()`, ...)
- an `axum::model_router(cool, codec, auth_provider)` constructor
- a `client::Client` constructor that wraps a `CratestackClient<C>`
- procedure dispatch glue and `events::Subscriptions` for `@@emit` model events
- a `schema_summary()` helper returning a `SchemaSummary` for tooling

The exact names depend on your schema; consult the root README for the canonical list.

## `include_client_macro!`

Emits a strict subset of the surface above — model and input types, enums, selection/projection helpers, and the `client::Client` wrapper — without any SQLx or Axum integration. Use this in consumer crates that talk to a CrateStack service but do not own the database.

```rust
use cratestack::include_client_macro;

include_client_macro!("../schemas/api.cstack");
```

## Decimal Backend

Generated code references `cratestack::Decimal`, which resolves at compile time to either `rust_decimal::Decimal` (`decimal-rust-decimal`, default) or the reserved `decimal-bigdecimal` backend.

## See Also

- `cratestack` — facade crate that re-exports both macros
- `cratestack-parser` — the parser the macros call
- [Quickstart](https://cratestack.dev/getting-started/quickstart)

## License

MIT
