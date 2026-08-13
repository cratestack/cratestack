# cratestack-api

The no-database server facade for CrateStack: Axum HTTP bindings, the
generated Rust client runtime, and the shared schema / parser / policy /
SQL surface — with `cratestack-sqlx` genuinely absent from the dependency
graph, not just switched off.

## When to use this crate

Pick `cratestack-api` for **procedures-only, no-database services**: a pure
RPC/REST facade in front of another service, a stateless computation
endpoint, or a gateway that only validates and forwards requests. Anything
whose schema declares `datasource { provider = "none" }` and has no `model`
blocks at all.

The moment a schema needs even one persisted `model`, switch to
[`cratestack-pg`](../cratestack-pg) instead — `datasource { provider =
"none" }` schemas can never declare a model (enforced at parse time,
[cratestack#327](https://github.com/cratestack/cratestack/issues/327)), and
this crate has no `cratestack-sqlx` dependency to serve one even if the
schema tried.

`cratestack-pg` with `default-features = false` also supports `db = None`
and continues to work — this crate doesn't replace that path, it just names
the "I never touch Postgres" case directly instead of asking a consumer to
depend on a crate named for the database backend they're explicitly opting
out of. See
[`docs/design/no-database-mode.md`](../../docs/design/no-database-mode.md)
for the full design and both entry points.

## Installation

Schema macros emit `::cratestack::*` paths. Alias this crate as
`cratestack` via Cargo's `package =` field:

```toml
[dependencies]
cratestack = { package = "cratestack-api", version = "0.6" }
```

Then in code:

```rust,ignore
cratestack::include_server_schema!("schema/foo.cstack", db = None);
```

`include_server_schema!(schema, db = Postgres)` under this crate fails to
compile — there is no `cratestack-sqlx`-backed `PgPool` type here to satisfy
the generated code. If a schema needs `db = Postgres`, depend on
`cratestack-pg` instead.

### Reproducing the `db = Postgres` compile error

`cratestack-macros`' `guard_server_postgres_backend` (cratestack#347) turns
what would otherwise be a wall of unrelated `E0432`/`E0433` "cannot find
`sqlx`/`SqlxRuntime`/`ModelDelegate` in `cratestack`" errors into one clear
message. This crate ships the fixture (`tests/fixtures/compile_fail_postgres.cstack`,
unused by any checked-in test — see below for why) to make this exact and
reproducible:

```cstack
datasource db {
  provider = "postgresql"
}

model Widget {
  id Int @id
  name String
}
```

Drop a file anywhere under `tests/` with:

```rust,ignore
cratestack::include_server_schema!("tests/fixtures/compile_fail_postgres.cstack", db = Postgres);
```

`cargo test -p cratestack-api --test <that file's name>` then fails with:

```text
error: include_server_schema!(..., db = Postgres) requires a facade crate with `cratestack-sqlx`
support, but `cratestack-macros` was compiled without its `postgres` feature — this facade
(e.g. `cratestack-api`) has no `cratestack-sqlx` dependency at all, under any feature, so there
is no `sqlx::PgPool`/`SqlxRuntime` for this schema's generated code to use. Depend on
`cratestack = { package = "cratestack-pg" }` instead for `db = Postgres` schemas, or switch this
schema to `datasource { provider = "none" }` + `db = None` if it never actually needs a database
(see https://github.com/cratestack/cratestack/issues/347).
```

This isn't wired up as a permanent `tests/*.rs` file: Cargo auto-discovers
every file under `tests/` as a test target and would try to compile it as
part of `cargo check --all-targets`/`just all-checks`, which is exactly what
this scenario should *not* do successfully. Same precedent as
`crates/cratestack-pg/tests/no_database_procedures.rs`'s doc comment, which
notes the equivalent negative case for
`guard_server_datasource_provider` — a `proc_macro::TokenStream`
compile-error path can't be exercised from a plain `cargo test` run, so it's
demonstrated manually instead.

## What this crate does not support

- **`db = Postgres`** — fails to compile; see above. This crate is
  `db = None`-only, by design, not by an accidentally-missing feature.
- **`transport grpc`** — `cratestack-grpc`/`prost` are not dependencies of
  this crate, so a `transport grpc` schema can't be compiled through
  `cratestack-api` at all, regardless of what the gRPC codegen itself
  supports (model CRUD and `procedure`s, unary or server-streaming, are
  both wired in — see `cratestack-grpc`'s README). `transport rpc` and REST
  (the default) both work fully. `transport grpc` is also planned for
  removal repo-wide in v0.9, so this gap isn't going to be closed.
- **Database migrations** — there's no database to migrate.

## Features

- `decimal-rust-decimal` *(default)* — `Decimal`-typed procedure args/returns
  use `rust_decimal`. Forwards to `cratestack-core`/`cratestack-sql`/
  `cratestack-client-rust`/`cratestack-macros` — no sqlx-backed half to
  gate, unlike `cratestack-pg`'s same-named feature.
- `decimal-bigdecimal` — arbitrary-precision `bigdecimal` backend instead
  (heap-allocated, not `Copy` — see `cratestack-core`'s README for the
  trait differences). Mutually exclusive with `decimal-rust-decimal`;
  selecting both is a compile error. Selecting *neither* is allowed as of
  cratestack#521 — the `Decimal` type is simply not exported, so only a schema
  that actually uses a `Decimal` field fails, with rustc's own "cannot find
  type `Decimal`". **Wire compatibility
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
  client runtime, alongside CBOR.
