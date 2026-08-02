# `datasource none` — a procedures-only server mode with no database

Status: **implemented** across three stories under epic #326 ("No-database
procedures-only server mode"): #327 (parser/semantic-checker support),
#328 (macro codegen — `db = None`), #329 (Cargo-feature-gating `sqlx` out of
the dependency graph, migrating this repo's own examples off the
`connect_lazy` workaround), plus a direct follow-up, #347 (a dedicated
`cratestack-api` facade crate — see §7).
Scope: `cratestack-parser` grammar/semantic checks, `cratestack-macros`
`include_server_schema!` codegen, `cratestack-pg`'s Cargo feature surface,
`cratestack-api`'s Cargo dependency surface.
Tracking: epic #326; stories #327, #328, #329, #347.

## 1. The idea

Not every CrateStack server owns a database. A pure RPC facade in front of
another service, a stateless computation endpoint, or a gateway that only
validates and forwards requests has no models to persist — but until this
epic, `include_server_schema!` still forced every such service through
`db = Postgres`: a real `datasource { provider = "postgresql" }` block, a
connection string, and (in this repo's own examples) a
`PgPoolOptions::new().connect_lazy(&url)` call whose only purpose was to
satisfy `Cratestack::builder(pool)`'s signature without actually opening a
socket. That's a workaround standing in for a feature, and it still pulled
`sqlx` into the binary for services that never touch a row.

`datasource { provider = "none" }` makes "no database" a first-class,
declared fact about the schema, the same way `provider = "postgresql"` or
`provider = "sqlite"` already are:

```cstack
datasource db {
  provider = "none"
}

type PingArgs {
  message String
}

type PingReply {
  echo String
}

procedure ping(args: PingArgs): PingReply
  @allow(auth() != null)
```

## 2. What it gives up — no models, ever

A schema declaring `datasource { provider = "none" }` can **never** declare
a `model` block. This is enforced at semantic-check time in
`cratestack-parser` (#327) — a `model` under `provider = "none"` is a parse-
time error, not a runtime one, and not something a later `model` addition
can silently start working around. The reasoning is structural, not just a
policy choice: a model needs somewhere to live (a table, a row shape, a
primary key), and "no database" means there is provably nowhere for that to
be. Because of this guarantee, the codegen and dependency-graph layers
below can treat "no models" as a fact they can build on, not merely a
convention to respect.

Concretely, under `db = None`:

- `ModelRouterState`, `model_router`, and the generated events module are
  omitted entirely from the macro's output — not emitted-empty, not
  emitted-and-unreachable. There is no `Cratestack::events()` accessor and
  no REST/RPC model routes, because there is provably nothing to route to.
- `Cratestack::builder()` takes **zero parameters** — no `PgPool`, no
  connection string, no `Option<PgPool>` that happens to always be `None`.
  `Cratestack` itself is a zero-field marker type (`#[derive(Clone, Copy)]
  pub struct Cratestack;`) with no `.pool()` method and no way to reach a
  `PgPool` from it at all.
- Procedure dispatch and policy evaluation (`ProcedureRegistry`,
  `authorize_with_db`) are unaffected — procedures and their `@allow`
  policies never depended on a database connection to begin with, so this
  is the one code path deliberately shared between `db = Postgres` and
  `db = None` rather than forked.

## 3. What it doesn't give up

- **`transport rpc` or REST** — either transport style works under
  `db = None`; this is orthogonal to the datasource. All five of this
  repo's own procedures-only examples (`rpc-procedures`, `rpc-batch`,
  `rpc-streaming`, `rpc-batch-debounce`) use `transport rpc`, but nothing
  about `db = None` requires it.
- **`type`/`enum`/`procedure` declarations, `@allow` policies, `@stream`,
  auth providers, custom codecs** — all of the framework's non-model
  surface is fully available. Procedure args/returns can even use the
  `Json` scalar type (`cratestack::Json<T>`) — see §4, it just resolves to
  a different, sqlx-free concrete type when the `postgres` feature is off.
- **`include_client_schema!`** — a client depending on a `db = None`
  service's schema is unaffected; client codegen was never
  datasource-dependent.

## 4. The dependency graph is database-free too (#329)

Stories #327/#328 made the generated *code* database-free; by themselves
they don't remove `sqlx` from what actually gets compiled — `cratestack-pg`
(the `cratestack = { package = "cratestack-pg" }` facade) still
unconditionally depended on and re-exported `cratestack-sqlx`. #329 closes
that gap with a Cargo feature:

```toml
[features]
default = ["postgres", "decimal-rust-decimal", "codec-json"]
postgres = ["dep:cratestack-sqlx"]

[dependencies]
cratestack-sqlx = { workspace = true, optional = true }
```

`postgres` is **default-on** — every existing `db = Postgres` consumer sees
zero change. A consumer that only ever declares `db = None` schemas opts
out explicitly:

```toml
[dependencies]
cratestack = { package = "cratestack-pg", version = "0.6", default-features = false }
```

With `postgres` disabled, `sqlx`/`cratestack-sqlx` are not compiled at all
— not linked-but-unused, genuinely absent from the dependency graph. The
`Json` scalar re-export switches from `cratestack_sqlx::sqlx::types::Json`
(needed for `sqlx::FromRow` to decode Postgres `jsonb` columns — a model-
only concern) to `cratestack_core::json::Json`, a plain serde newtype that
covers everything a `db = None` schema's procedure args/returns need. This
works cleanly *because* of §2's guarantee: models (the only consumer of
the sqlx-flavored `Json`) can never exist under `db = None`, so there is
never a case where a `postgres`-disabled build needs the sqlx-backed type.

See `examples/no-database-verification` for a real `cargo tree` proof of
absence/presence, and its README for why that proof has to live in a
standalone crate outside this repo's own workspace (Cargo unifies a shared
dependency's features across every workspace member building it in the
same session, which would otherwise mask the gate for an in-workspace
example even though it works correctly for an external consumer).

## 5. This repo's own examples

`examples/rpc-procedures`, `rpc-batch`, `rpc-streaming`, and
`rpc-batch-debounce` are genuinely procedures-only (verified: zero `model`
blocks in any of their schemas) and depend on `cratestack-api` (§7) — the
first-class no-database facade. `examples/microservice-pair` keeps
`db = Postgres` deliberately — its `orders.cstack` schema owns a real
`Order` model; the `connect_lazy` call in that example is confined to its
own `router_builds_offline` test (an offline router-construction smoke
test), not a stand-in for the whole service being database-free.

## 6. When to use `db = None`

Reach for it when a service's schema has no `model` blocks at all — pure
RPC/REST procedure gateways, computation-only endpoints, or a
transformation layer in front of another service's data. If a schema needs
even one persisted model, it needs `db = Postgres` (or, for embedded/wasm
targets, `include_embedded_schema!`'s rusqlite backend) — `db = None` is
not a "start here and add models later" default; adding a model to a
`provider = "none"` schema is a parse error by design (§2).

## 7. `cratestack-api` — a facade named for what it is (#347)

§4 closed the `sqlx` dependency-graph gap with a Cargo feature: a `db =
None`-only consumer depends on `cratestack-pg` (the crate literally named
for the Postgres backend) with `default-features = false` to turn that
backend off. That works, but it reads backwards — a service that, by
definition, never touches Postgres shouldn't have to depend on a crate
named "pg" and then switch the pg part off to prove it.

`cratestack-api` (`crates/cratestack-api`) is a **third facade**, following
the exact structural pattern `cratestack-pg`/`cratestack-sqlite` already
established: its own `Cargo.toml`, its own `[lib] name = "cratestack"`
rename trick, its own `src/lib.rs` re-exporting the shared schema / parser
/ policy / SQL surface plus `cratestack-axum` and `cratestack-client-rust`.
It is not a thin wrapper around `cratestack-pg` — there is no shared
"backend" trait between any of the three facades, by the same deliberate
choice documented in `cratestack-pg`/`cratestack-sqlite`'s own doc comments.

The difference from `cratestack-pg` is structural, not a feature flag:
`cratestack-api`'s `Cargo.toml` has no `cratestack-sqlx` entry in
`[dependencies]` at all, optional or otherwise, and no `postgres` feature to
gate one. Since `datasource { provider = "none" }` schemas can never
declare a `model` (§2), and `db = Postgres` codegen is the only path that
ever references sqlx-backed symbols, a facade that structurally never has
`cratestack-sqlx` available can only ever support `db = None` — which is
exactly this crate's scope, not a limitation to work around. A schema
compiled with `include_server_schema!(schema, db = Postgres)` under
`cratestack-api` fails to compile with a single clear `compile_error!`
(`cratestack-macros`' `guard_server_postgres_backend`, mirroring the
existing `guard_server_grpc_transport`/`cfg!(feature = "grpc")` mechanism)
instead of a wall of unrelated "cannot find `sqlx`/`SqlxRuntime` in
`cratestack`" resolution errors. `cratestack-api` also omits
`cratestack-grpc`/`prost`: `transport grpc` codegen is entirely
model-driven, so it could only ever produce a zero-method service under
`db = None` — there is nothing for gRPC to add here.

**Both entry points are supported and neither is deprecated:**

```toml
# Recommended for new `db = None` services — named for what it is.
cratestack = { package = "cratestack-api", version = "0.6" }
```

```toml
# Still works, unchanged, for existing consumers of this pattern (#329).
cratestack = { package = "cratestack-pg", version = "0.6", default-features = false }
```

Pick `cratestack-api` for a new procedures-only service. If a project
already depends on `cratestack-pg` with `default-features = false`, there is
no requirement to migrate — nothing about that pattern changes or is
scheduled for removal. This repo's own four procedures-only examples
(`rpc-procedures`, `rpc-batch`, `rpc-streaming`, `rpc-batch-debounce`) have
migrated to `cratestack-api` (§5); see `crates/cratestack-api/README.md` and
`examples/no-database-verification-api` for the crate's own docs and
dependency-graph proof.
